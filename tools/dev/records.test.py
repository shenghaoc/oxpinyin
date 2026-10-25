#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Round-trip and shape regressions for shared resource record types."""
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from typing import NotRequired
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host, records, slot

WHEN = '2026-10-25T00:00:00Z'


class RecordShapeTests(unittest.TestCase):
    def test_resource_snapshot_requires_every_read_field(self):
        self.assertEqual(records.ResourceSnapshot.__required_keys__, frozenset({
            'available_mib', 'memory_limit_mib', 'total_mib', 'cpus', 'cargo_cpus', 'notes'}))
        self.assertEqual(records.ResourceSnapshot.__optional_keys__, frozenset())
        for name in ('available_mib', 'memory_limit_mib', 'total_mib'):
            with self.subTest(name=name):
                self.assertEqual(records.ResourceSnapshot.__annotations__[name], records.MiB | None)

    def test_budgeted_resources_add_the_reservation_totals(self):
        self.assertEqual(records.BudgetedResources.__required_keys__, frozenset({
            'available_mib', 'memory_limit_mib', 'total_mib', 'cpus', 'cargo_cpus', 'notes',
            'slot_reserved_mib', 'container_reserved_mib'}))
        self.assertEqual(records.BudgetedResources.__optional_keys__, frozenset())

    def test_holder_estimate_is_optional_and_measured_in_mib(self):
        self.assertEqual(records.HeavyHolder.__optional_keys__, frozenset({'estimate_mib'}))
        self.assertNotIn('estimate_mib', records.HeavyHolder.__required_keys__)
        self.assertEqual(records.HeavyHolder.__annotations__['estimate_mib'],
                         NotRequired[records.MiB])

    def test_container_record_requires_every_reservation_field(self):
        self.assertEqual(records.ContainerReservationRecord.__required_keys__, frozenset({
            'schema', 'runtime', 'kind', 'name', 'token', 'memory_mib', 'phase', 'since'}))
        self.assertEqual(records.ContainerReservationRecord.__optional_keys__, frozenset())
        self.assertEqual(records.ContainerReservationRecord.__annotations__['memory_mib'], records.MiB)


class RoundTripTests(unittest.TestCase):
    def test_resource_snapshot_keeps_unknown_and_known_zero_distinct(self):
        snapshot = records.ResourceSnapshot(
            available_mib=0, memory_limit_mib=0, total_mib=None,
            cpus=1, cargo_cpus=1, notes=['synthetic known zero'])
        loaded = json.loads(json.dumps(snapshot))
        self.assertEqual(loaded, snapshot)
        self.assertIsNotNone(loaded['available_mib'])
        self.assertEqual(loaded['available_mib'], 0)
        self.assertIsNotNone(loaded['memory_limit_mib'])
        self.assertEqual(loaded['memory_limit_mib'], 0)
        self.assertIsNone(loaded['total_mib'])

    def test_budgeted_resources_keep_unknown_and_known_reservation_totals(self):
        for containers in (0, None):
            with self.subTest(containers=containers):
                resources = records.BudgetedResources(
                    available_mib=0, memory_limit_mib=None, total_mib=None,
                    cpus=1, cargo_cpus=1, notes=['synthetic reservation totals'],
                    slot_reserved_mib=0, container_reserved_mib=containers)
                loaded = json.loads(json.dumps(resources))
                self.assertEqual(loaded, resources)
                self.assertIsNotNone(loaded['slot_reserved_mib'])
                self.assertEqual(loaded['slot_reserved_mib'], 0)
                if containers is None:
                    self.assertIsNone(loaded['container_reserved_mib'])
                else:
                    self.assertIsNotNone(loaded['container_reserved_mib'])
                    self.assertEqual(loaded['container_reserved_mib'], 0)

    def test_provisional_holder_round_trips_without_estimate(self):
        holder = records.HeavyHolder(pid=1234, worktree='/checkout', command='cargo test',
                                     since=WHEN, token='holder-token')
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'heavy-slot-0'
            with path.open('w+') as file:
                host.write_holder(file, holder)
            loaded = host.read_holder(path)
        self.assertEqual(loaded, holder)
        self.assertNotIn('estimate_mib', loaded)

    def test_holder_round_trips_a_known_zero_and_a_positive_estimate(self):
        holder = records.HeavyHolder(pid=1234, worktree='/checkout', command='cargo test',
                                     since=WHEN, token='holder-token')
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'heavy-slot-0'
            for estimate in (0, 2400):
                with self.subTest(estimate=estimate):
                    held = dict(holder, estimate_mib=estimate)
                    with path.open('w+') as file:
                        host.write_holder(file, held)
                    loaded = host.read_holder(path)
                    self.assertEqual(loaded, held)
                    self.assertIsNotNone(loaded['estimate_mib'])
                    self.assertEqual(loaded['estimate_mib'], estimate)

    def test_container_record_round_trips_through_the_atomic_writer(self):
        record = records.ContainerReservationRecord(
            schema=1, runtime='/usr/bin/docker', kind='docker', name='test-container',
            token='reservation-token', memory_mib=2560, phase='creating', since=WHEN)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'container-memory-token.json'
            slot.write_container_record(path, record)
            self.assertEqual(host.read_holder(path), record)
            self.assertEqual(json.loads(path.read_text()), record)


class InvalidRecordTests(unittest.TestCase):
    def test_malformed_holder_remains_unreadable(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'heavy-slot-0'
            path.write_text('{')
            self.assertIsNone(host.read_holder(path))

    def test_provisional_and_invalid_estimates_reserve_nothing(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            path = directory / 'heavy-slot-1'
            with path.open('a+') as file:
                slot.lock_file(file, slot.LOCK_EX | slot.LOCK_NB)
                for estimate in (None, 0, -1, True, '2400'):
                    with self.subTest(estimate=estimate):
                        holder = dict(pid=os.getpid())
                        if estimate is not None:
                            holder['estimate_mib'] = estimate
                        host.write_holder(file, holder)
                        self.assertEqual(slot.other_reservations(directory, 0), 0)

    def test_invalid_container_record_still_makes_memory_unknown(self):
        valid = records.ContainerReservationRecord(
            schema=1, runtime='/usr/bin/docker', kind='docker', name='test-container',
            token='reservation-token', memory_mib=2560, phase='creating', since=WHEN)
        invalid = {
            'schema': dict(valid, schema=2),
            'zero memory': dict(valid, memory_mib=0),
            'boolean memory': dict(valid, memory_mib=True),
            'unknown memory': dict(valid, memory_mib=None),
            'string memory': dict(valid, memory_mib='2560'),
            'phase': dict(valid, phase='unknown'),
            'kind': dict(valid, kind='other'),
            'relative runtime': dict(valid, runtime='docker'),
            'empty name': dict(valid, name=''),
            'empty token': dict(valid, token=''),
        }
        for label, record in invalid.items():
            with self.subTest(label), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                (directory / 'container-memory-test.json').write_text(json.dumps(record))
                out = io.StringIO()
                self.assertIsNone(slot.container_reservations(directory, out=out))
                self.assertIn('container reservation invalid', out.getvalue())

    def test_invalid_container_json_still_makes_memory_unknown(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / 'container-memory-test.json').write_text('{')
            out = io.StringIO()
            self.assertIsNone(slot.container_reservations(directory, out=out))
            self.assertIn('container reservation invalid', out.getvalue())


if __name__ == '__main__':
    unittest.main()
