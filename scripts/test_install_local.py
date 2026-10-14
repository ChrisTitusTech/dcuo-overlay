#!/usr/bin/env python3
"""Temporary-prefix installer tests; never touch the caller's installation."""
import json
from pathlib import Path
import stat
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
import install_local as installer


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.prefix = self.root / "prefix with spaces"
        self.binary = self.root / "binary"
        self.binary.write_bytes(b"synthetic executable version one")
        self.binary.chmod(0o755)
        self.desktop = self.root / "template.desktop"
        self.desktop.write_text("[Desktop Entry]\nType=Application\nName=Test\nExec=dcuo-companion\nIcon=dcuo-companion\n")
        self.icon = self.root / "icon.svg"
        self.icon.write_text('<svg xmlns="http://www.w3.org/2000/svg"/>')
        self.addCleanup(self.temporary.cleanup)

    def run_operation(self, action, **options):
        return installer.operate(action, self.prefix, self.binary, self.desktop, self.icon, **options)

    def snapshot(self):
        if not self.prefix.exists():
            return {}
        return {str(path.relative_to(self.prefix)): (path.read_bytes(), stat.S_IMODE(path.stat().st_mode))
                for path in self.prefix.rglob("*") if path.is_file()}

    def test_install_upgrade_rollback_remove_preserves_settings_maps_and_other_files(self):
        self.prefix.mkdir()
        unrelated = self.prefix / "share/dcuo-companion/maps/private-map"
        unrelated.parent.mkdir(parents=True)
        unrelated.write_bytes(b"user map")
        config = self.root / "config/settings.json"
        config.parent.mkdir()
        config.write_bytes(b"user settings")
        self.run_operation("install", position=(80, 90))
        executable = self.prefix / "bin/dcuo-companion"
        old = executable.read_bytes()
        desktop = (self.prefix / "share/applications/dcuo-companion.desktop").read_text()
        self.assertIn('Exec="' + str(executable) + '" --position 80 90', desktop)
        self.assertIn("Icon=" + str(self.prefix), desktop)
        manifest = self.prefix / installer.MANIFEST
        self.assertEqual(stat.S_IMODE(manifest.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(manifest.parent.stat().st_mode), 0o700)
        self.binary.write_bytes(b"synthetic executable version two")
        self.run_operation("upgrade", position=(100, 120))
        self.assertEqual(executable.read_bytes(), self.binary.read_bytes())
        self.run_operation("rollback")
        self.assertEqual(executable.read_bytes(), old)
        self.assertIn("--position 80 90", (self.prefix / "share/applications/dcuo-companion.desktop").read_text())
        self.run_operation("uninstall")
        self.assertFalse(executable.exists())
        self.assertFalse(manifest.exists())
        self.assertEqual(unrelated.read_bytes(), b"user map")
        self.assertEqual(config.read_bytes(), b"user settings")

    def test_only_one_previous_version_is_retained_and_rollback_can_be_undone(self):
        self.run_operation("install")
        self.binary.write_bytes(b"two")
        self.run_operation("upgrade")
        self.binary.write_bytes(b"three")
        self.run_operation("upgrade")
        self.run_operation("rollback")
        self.assertEqual((self.prefix / "bin/dcuo-companion").read_bytes(), b"two")
        self.run_operation("rollback")
        self.assertEqual((self.prefix / "bin/dcuo-companion").read_bytes(), b"three")
        backup = self.prefix / installer.STATE / "previous"
        self.assertEqual(len(list(backup.iterdir())), 3)

    def test_modified_owned_file_refuses_every_operation_before_mutation(self):
        self.run_operation("install")
        (self.prefix / "bin/dcuo-companion").write_bytes(b"user modification")
        before = self.snapshot()
        for action in ["upgrade", "uninstall", "rollback"]:
            with self.assertRaises(installer.InstallError):
                self.run_operation(action)
            self.assertEqual(self.snapshot(), before)

    def test_unknown_targets_and_missing_prebuilt_binary_are_not_overwritten(self):
        executable = self.prefix / "bin/dcuo-companion"
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"unowned")
        before = self.snapshot()
        with self.assertRaises(installer.InstallError):
            self.run_operation("install")
        self.assertEqual(self.snapshot(), before)
        self.binary.unlink()
        with self.assertRaises(installer.InstallError):
            self.run_operation("install")
        self.assertEqual(self.snapshot(), before)

    def test_symlink_ancestors_targets_and_traversal_are_refused(self):
        outside = self.root / "outside"
        outside.mkdir()
        self.prefix.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(installer.InstallError):
            self.run_operation("install")
        self.assertEqual(list(outside.iterdir()), [])
        self.prefix.unlink()
        self.prefix.mkdir()
        (self.prefix / "bin").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(installer.InstallError):
            self.run_operation("install")
        (self.prefix / "bin").unlink()
        with self.assertRaises(installer.InstallError):
            installer.operate("install", self.prefix / "../escape", self.binary, self.desktop, self.icon)
        self.assertEqual(list(outside.iterdir()), [])

    def test_backup_change_and_manifest_duplicate_or_unknown_paths_are_refused(self):
        self.run_operation("install")
        self.run_operation("upgrade")
        backup = self.prefix / next(iter(installer.BACKUPS.values()))
        backup.write_bytes(b"changed backup")
        before = self.snapshot()
        with self.assertRaises(installer.InstallError):
            self.run_operation("rollback")
        self.assertEqual(self.snapshot(), before)
        manifest = self.prefix / installer.MANIFEST
        state = json.loads(manifest.read_text())
        state["files"]["../escape"] = state["files"].pop("bin/dcuo-companion")
        manifest.write_text(json.dumps(state))
        with self.assertRaises(installer.InstallError):
            self.run_operation("uninstall")
        manifest.write_text('{"version":1,"version":1,"files":{},"previous":null}')
        with self.assertRaises(installer.InstallError):
            self.run_operation("uninstall")

    def test_failed_upgrade_restores_every_completed_write(self):
        self.run_operation("install")
        self.binary.write_bytes(b"two")
        before = self.snapshot()
        original = installer.os.replace
        for fail_at in [1, 5, 7]:
            calls = 0

            def fail_selected(source, destination):
                nonlocal calls
                calls += 1
                if calls == fail_at:
                    raise OSError("synthetic disk failure")
                return original(source, destination)

            with patch.object(installer.os, "replace", side_effect=fail_selected):
                with self.assertRaisesRegex(installer.InstallError, "previous files restored"):
                    self.run_operation("upgrade")
            self.assertEqual(self.snapshot(), before)
            self.assertFalse(any(path.name.startswith(".dcuo-install-") for path in self.prefix.rglob("*")))

    def test_failed_uninstall_restores_removed_owned_files(self):
        self.run_operation("install")
        before = self.snapshot()
        original = Path.unlink
        calls = 0

        def fail_second(path, *args, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("synthetic removal failure")
            return original(path, *args, **kwargs)

        with patch.object(Path, "unlink", fail_second):
            with self.assertRaisesRegex(installer.InstallError, "previous files restored"):
                self.run_operation("uninstall")
        self.assertEqual(self.snapshot(), before)

    def test_failed_initial_install_removes_its_files_and_new_directories(self):
        original = installer.os.replace
        calls = 0

        def fail_second(source, destination):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("synthetic failure")
            return original(source, destination)

        with patch.object(installer.os, "replace", side_effect=fail_second):
            with self.assertRaises(installer.InstallError):
                self.run_operation("install")
        self.assertFalse(self.prefix.exists())

    def test_adoption_requires_exact_contents_and_modes_without_overwriting(self):
        payload = installer.source_payload(self.prefix, self.binary, self.desktop, self.icon, (80, 80))
        for name, (data, mode) in payload.items():
            path = self.prefix / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            path.chmod(mode)
        inode = (self.prefix / "bin/dcuo-companion").stat().st_ino
        with self.assertRaises(installer.InstallError):
            self.run_operation("install", adopt=True, position=(10, 10))
        self.assertFalse((self.prefix / installer.MANIFEST).exists())
        self.run_operation("install", adopt=True, position=(80, 80))
        self.assertEqual((self.prefix / "bin/dcuo-companion").stat().st_ino, inode)
        self.run_operation("uninstall")

    def test_desktop_path_special_characters_and_invalid_inputs(self):
        quoted = installer.quote_exec(Path('/tmp/with "quotes" and $dollar`tick\\slash%/bin'))
        self.assertIn('\\\\"quotes\\\\"', quoted)
        self.assertIn('\\\\$dollar', quoted)
        self.assertIn('\\\\`tick', quoted)
        self.assertIn('\\\\\\\\slash%%', quoted)
        with self.assertRaises(installer.InstallError):
            installer.quote_exec(Path("/tmp/bad=path/bin"))
        with self.assertRaises(installer.InstallError):
            installer.quote_exec(Path("/tmp/new\nline/bin"))
        with self.assertRaises(installer.InstallError):
            self.run_operation("install", position=(100_001, 0))
        self.assertFalse(self.prefix.exists())

    def test_nonregular_oversized_or_nonexecutable_sources_are_refused(self):
        self.binary.chmod(0o644)
        with self.assertRaises(installer.InstallError):
            self.run_operation("install")
        self.binary.chmod(0o755)
        with self.binary.open("wb") as stream:
            stream.truncate(installer.TARGETS["bin/dcuo-companion"][0] + 1)
        with self.assertRaises(installer.InstallError):
            self.run_operation("install")
        self.assertFalse(self.prefix.exists())


if __name__ == "__main__":
    unittest.main()
