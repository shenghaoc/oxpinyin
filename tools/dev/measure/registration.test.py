"""Native bench registrations preserve every unregistered observation check."""
import copy
import gzip
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tools.dev.measure.registration import Registration


class ObservationRegistration(unittest.TestCase):
    def setUp(self):
        self.binding = dict(base_commit='a' * 40, pr_tree='b' * 40)
        base = dict(verify=dict(documents=[
            dict(startup='units', digest='unchanged'),
            dict(startup='transcript', parser='full', input='nihao', parsed=5,
                 candidates=['猊', '鲵'], count=2)], stderr_hex='', userfiles={'user.conf': 'same'}),
            persist=dict(documents=[dict(startup='persist', op='save', value=True)],
                         stderr_hex='', userfiles={'user.bin': 'same'}),
            unit_stderr_hex='', unit_userfiles={'user.conf': 'same'})
        pr = copy.deepcopy(base)
        pr['verify']['documents'][1]['candidates'].reverse()
        self.observations = dict(base=base, pr=pr, control=copy.deepcopy(pr), pin=copy.deepcopy(base))

    def recorded(self):
        record = Registration(self.binding, recording=True)
        self.assertEqual(record.check('startup', 'pinyin', 'phrases', self.observations), 1)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'registration.jsonl.gz'
            self.assertEqual(record.write(path), 1)
            loaded = Registration.load(path, self.binding)
            self.assertEqual(len(loaded.entries), 1)
            with gzip.open(path, 'rt', encoding='utf-8') as source:
                document = json.loads(next(source))
                document['entries'] = [json.loads(line) for line in source]
                return document

    def test_matching_registration_contains_complete_base_and_pr(self):
        document = self.recorded()
        entry = document['entries'][0]
        self.assertIn('persist', entry['base'])
        self.assertIn('unit_userfiles', entry['pr'])
        registration = Registration(self.binding, document=document)
        self.assertEqual(registration.check('startup', 'pinyin', 'phrases', self.observations), 1)

    def test_unregistered_difference_stops(self):
        registration = Registration(self.binding, document=self.recorded())
        changed = copy.deepcopy(self.observations)
        for arm in ('pr', 'control'):
            changed[arm]['verify']['documents'][1]['candidates'][0] = 'another answer'
        with self.assertRaisesRegex(ValueError, 'unregistered'):
            registration.check('startup', 'pinyin', 'phrases', changed)

    def test_stale_base_or_pr_tree_stops(self):
        document = self.recorded()
        for field in ('base_commit', 'pr_tree'):
            binding = dict(self.binding, **{field: 'c' * 40})
            with self.assertRaisesRegex(ValueError, 'stale'):
                Registration(binding, document=document)

    def test_pr_control_mismatch_stops_even_in_record_mode(self):
        changed = copy.deepcopy(self.observations)
        changed['control']['verify']['documents'][1]['candidates'].reverse()
        with self.assertRaisesRegex(ValueError, 'PR/control'):
            Registration(self.binding, recording=True).check('startup', 'pinyin', 'phrases', changed)

    def test_persistence_files_stderr_units_and_parse_results_cannot_be_registered(self):
        for field in ('persist', 'files', 'stderr', 'units', 'parsed'):
            changed = copy.deepcopy(self.observations)
            for arm in ('pr', 'control'):
                value = changed[arm]
                if field == 'persist':
                    value['persist']['documents'][0]['value'] = False
                elif field == 'files':
                    value['persist']['userfiles']['user.bin'] = 'changed'
                elif field == 'stderr':
                    value['verify']['stderr_hex'] = '01'
                elif field == 'units':
                    value['verify']['documents'][0]['digest'] = 'changed'
                else:
                    value['verify']['documents'][1]['parsed'] = 4
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'protected'):
                Registration(self.binding, recording=True).check('startup', 'pinyin', 'phrases', changed)


if __name__ == '__main__':
    unittest.main()
