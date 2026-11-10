#!/usr/bin/env python3
"""Exercise release/version contracts without credentials or remote writes."""

import json
import os
import pathlib
import re
import runpy
import shutil
import subprocess
import tempfile
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent


def run(*args, cwd=ROOT, **kwargs):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True, **kwargs)


class ReleaseChecks(unittest.TestCase):
    def test_workflow_boundaries(self):
        workflows = {p.name: p.read_text() for p in (ROOT / ".github/workflows").glob("*.yml")}
        for name, text in workflows.items():
            self.assertNotRegex(text, r"(?m)^\s+tags:", name)
            self.assertNotIn("cargo publish", text, name)
            for action in re.findall(r"(?m)^\s*(?:-\s*)?uses: ([^\s]+)", text):
                if not action.startswith("./"):
                    self.assertRegex(action, r"@[0-9a-f]{40}$", name)
            for checkout in text.split("uses: actions/checkout@")[1:]:
                self.assertIn("persist-credentials: false", checkout.split("\n      - ")[0], name)
        callers = {name for name, text in workflows.items() if "uses: ./.github/workflows/publish-release.yml" in text}
        self.assertEqual(callers, {"release-pr.yml", "release-tag.yml", "recover-release.yml"})
        publishers = {name for name, text in workflows.items() if "publish: true" in text}
        self.assertEqual(publishers, {"release-tag.yml", "recover-release.yml"})
        publisher = workflows["publish-release.yml"]
        self.assertLess(publisher.index("require-tag"), publisher.index("name: Create chaba2-bot token"))
        self.assertIn("publish: false", workflows["release-pr.yml"])
        self.assertIn("git add changelog.md Cargo.toml Cargo.lock fuzz/Cargo.lock", workflows["release-pr.yml"])
        self.assertIn("      - fuzz/Cargo.lock", workflows["release-pr.yml"])
        self.assertIn("needs: tag-release", workflows["release-tag.yml"])
        self.assertIn("needs: [validate-release, dry-run-release, create-tag]", workflows["recover-release.yml"])

    def test_recovery_can_replace_its_dry_run_artifact(self):
        recovery = (ROOT / ".github/workflows/recover-release.yml").read_text()
        publisher = (ROOT / ".github/workflows/publish-release.yml").read_text()
        self.assertEqual(recovery.count("uses: ./.github/workflows/publish-release.yml"), 2)
        self.assertIn("needs: [validate-release, dry-run-release, create-tag]", recovery)
        upload = publisher.split("uses: actions/upload-artifact@", 1)[1].split("\n  release:", 1)[0]
        self.assertIn("overwrite: true", upload)
        artifact = "name: release-source-${{ inputs.checkout_ref }}"
        self.assertIn(artifact, upload)
        self.assertIn(artifact, publisher.split("uses: actions/download-artifact@", 1)[1])

    def test_shared_version_and_invalid_input(self):
        update = runpy.run_path(str(ROOT / "scripts/set-release-version.py"))["update"]
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "fuzz").mkdir()
            for name in ["Cargo.toml", "Cargo.lock", "fuzz/Cargo.lock"]:
                shutil.copyfile(ROOT / name, root / name)
            old = {name: tomllib.loads((root / name).read_text())
                   for name in ["Cargo.lock", "fuzz/Cargo.lock"]}
            for invalid in ["v1.2.3", "1.2", "01.2.3", "1.2.3-rc.1", "1.2.3\n"]:
                with self.assertRaises(ValueError):
                    update(root, invalid)
            for name, lock in old.items():
                self.assertEqual(tomllib.loads((root / name).read_text()), lock)
            update(root, "1.2.3")
            update(root, "1.2.3")  # retry is idempotent
            manifest = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
            self.assertEqual(manifest["package"]["version"], "1.2.3")
            for name in ["cel-core", "cel-core-proto"]:
                self.assertEqual(manifest["dependencies"][name]["version"], "1.2.3")
            for filename, previous in old.items():
                lock = tomllib.loads((root / filename).read_text())
                for before, after in zip(previous["package"], lock["package"], strict=True):
                    expected = dict(before)
                    if before["name"] in ["cel-core", "cel-core-proto", "cel-core-conformance"]:
                        expected["version"] = "1.2.3"
                    self.assertEqual(after, expected)

    def test_status_requires_current_success_from_trusted_app(self):
        trusted = "release-fixture[bot]"
        success = {"context": "Release build dry run", "state": "success", "creator": {"login": trusted}}
        for statuses, expected in [
            ([success], True),
            ([], False),
            ([dict(success, state="pending"), success], False),
            ([dict(success, creator={"login": "other[bot]"})], False),
            ([dict(success, context="unrelated")], False),
        ]:
            result = run("bash", "scripts/verify-release-status.sh", trusted, input=json.dumps({"statuses": statuses}))
            self.assertEqual(result.returncode == 0, expected, result.stderr)

    def test_tag_provenance_and_git_cliff(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            env = dict(os.environ, GIT_AUTHOR_NAME="Release fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
                       GIT_COMMITTER_NAME="Release fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid")
            def git(*args):
                result = run("git", *args, cwd=root, env=env)
                self.assertEqual(result.returncode, 0, result.stderr)
                return result.stdout.strip()
            git("init", "-q")
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.5.1"\n')
            git("add", ".")
            git("commit", "-qm", "chore: fixture")
            sha = git("rev-parse", "HEAD")
            git("tag", "v0.5.1")
            for tag, commit, mode, expected in [
                ("v0.5.1", sha, "require-tag", True),
                ("v0.5.2", sha, "validate-ref", False),
                ("v00.5.1", sha, "validate-ref", False),
                ("v0.5.1", "0" * 40, "validate-ref", False),
                ("v0.5.1", sha, "unknown", False),
            ]:
                result = run("bash", str(ROOT / "scripts/verify-release-tag.sh"), tag, commit, mode, cwd=root)
                self.assertEqual(result.returncode == 0, expected, result.stderr)
            for title, expected in [("fix: repair", "v0.5.2"), ("feat: extend", "v0.6.0"), ("feat!: break", "v1.0.0")]:
                git("commit", "--allow-empty", "-qm", title)
                result = run("git", "cliff", "--config", str(ROOT / "cliff.toml"), "--bumped-version", cwd=root)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), expected)


if __name__ == "__main__":
    unittest.main()
