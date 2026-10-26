"""Run-input registrations made only from a bench's complete observations."""
import copy
import gzip
import json
import subprocess


def git_identity(base_checkout, pr_checkout):
    """Bind a clean PR checkout and the actual baseline checkout to Git objects."""
    def git(root, *args):
        return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()
    if (git(pr_checkout, 'status', '--porcelain') or
            git(base_checkout, 'status', '--porcelain')):
        raise ValueError('registration requires a clean committed PR tree')
    return dict(base_commit=git(base_checkout, 'rev-parse', 'HEAD'),
                pr_tree=git(pr_checkout, 'rev-parse', 'HEAD^{tree}'))


def protected(value):
    """Keep all checks except the explicitly registered ranking answers.

    Candidate rows, count and sentence texts may change in a native ranking
    transcript, including a read after reopen. Persist operations, file bytes,
    stderr, unit records and every other return/parse field remain exact.
    """
    if isinstance(value, list):
        return [protected(item) for item in value]
    if not isinstance(value, dict):
        return value
    ranking = (('input' in value and value.get('startup') in ('transcript', 'persist'))
               or value.get('startup') == 'after_units')
    # The bisect first-candidate observation has no transcript header.
    ranking |= set(('consumed', 'parsed', 'guessed', 'candidates')) <= value.keys()
    if value.get('startup') == 'units':
        ranking = False
    excluded = {'candidates', 'count', 'sentence', 'sentences'} if ranking else set()
    return {key: protected(item) for key, item in value.items() if key not in excluded}


def input_observations(observation):
    """Separate each input while retaining the complete protected envelope."""
    if 'verify' not in observation:
        return [('nihao', 0, observation)]
    def is_input(row):
        return 'input' in row or row.get('startup') == 'after_units'
    common = copy.deepcopy(observation)
    for stage in ('verify', 'persist'):
        common[stage]['documents'] = [row for row in observation[stage]['documents'] if not is_input(row)]
    occurrences = {}
    result = []
    for stage in ('verify', 'persist'):
        for row in observation[stage]['documents']:
            if not is_input(row):
                continue
            name = ':'.join((stage, row['startup'], row.get('parser', 'full'), row.get('input', 'nihao')))
            ordinal = occurrences.get(name, 0)
            occurrences[name] = ordinal + 1
            value = copy.deepcopy(common)
            value[stage]['documents'].append(row)
            result.append((name, ordinal, value))

    if not result:
        raise ValueError('verification supplied no input observations')
    return result


class Registration:
    """One shared matcher for recording and measuring native bench inputs."""
    def __init__(self, binding, *, recording=False, document=None):
        self.binding = copy.deepcopy(binding)
        self.recording = recording
        self.entries = {}
        if document is not None:
            if document.get('schema') != 2 or document.get('binding') != binding:
                raise ValueError('stale registration base commit, PR tree, libraries or inputs')
            for entry in document['entries']:
                key = tuple(entry['key'])
                if key in self.entries or set(entry) != {'key', 'base', 'pr', 'pin'}:
                    raise ValueError('invalid or repeated registration entry')
                self.entries[key] = copy.deepcopy(entry)

    @classmethod
    def load(cls, path, binding):
        # A complete file envelope is repeated in each input observation.
        # Stream entries and share equal immutable byte strings so neither a
        # giant decoded JSON string nor repeated file bytes fill the host RAM.
        strings = {}
        def share_bytes(value):
            for key in ('raw_hex', 'stderr_hex', 'unit_stderr_hex'):
                text = value.get(key)
                if isinstance(text, str):
                    value[key] = strings.setdefault(text, text)
            return value
        with gzip.open(path, 'rt', encoding='utf-8') as source:
            document = json.loads(next(source))
            document['entries'] = (json.loads(line, object_hook=share_bytes) for line in source)
            return cls(binding, document=document)

    def check(self, tool, facade, profile, observations):
        from tools.dev.measure.startup import first_difference

        if observations['pr'] != observations['control']:
            raise ValueError('PR/control observation mismatch: ' + repr(
                first_difference(observations['pr'], observations['control'])))
        projected = {arm: input_observations(row) for arm, row in observations.items()}
        identities = [[(name, ordinal) for name, ordinal, _ in rows] for rows in projected.values()]
        if any(identity != identities[0] for identity in identities):
            raise ValueError('input identity mismatch')
        accepted = 0
        for index, (name, ordinal) in enumerate(identities[0]):
            values = {arm: rows[index][2] for arm, rows in projected.items()}
            baseline = protected(values['base'])
            for arm, value in values.items():
                difference = first_difference(baseline, protected(value))
                if difference is not None:
                    raise ValueError('protected persistence, file, stderr, unit or return observation differs: '
                                     + repr((tool, facade, profile, name, ordinal, arm, difference)))
            if all(value == values['base'] for value in values.values()):
                continue
            key = (tool, facade, profile, name, ordinal)
            entry = dict(key=list(key), base=values['base'], pr=values['pr'], pin=values['pin'])
            if self.recording:
                previous = self.entries.setdefault(key, copy.deepcopy(entry))
                if previous != entry:
                    raise ValueError('record observations are not repeatable')
            elif self.entries.get(key) != entry:
                raise ValueError('unregistered observation difference: ' + repr(key))
            accepted += 1
        return accepted

    def write(self, path):
        if not self.recording:
            raise ValueError('measure mode cannot write a registration')
        # A failed/uncertain earlier record is never silently overwritten.
        # Every entry still holds complete base/PR/pin observations, rather
        # than references or an allowlist. Compression only changes storage.
        with gzip.open(path, 'xt', encoding='utf-8') as destination:
            json.dump(dict(schema=2, binding=self.binding), destination, sort_keys=True)
            destination.write('\n')
            for key in sorted(self.entries):
                json.dump(self.entries[key], destination, ensure_ascii=False, sort_keys=True)
                destination.write('\n')
        return len(self.entries)
