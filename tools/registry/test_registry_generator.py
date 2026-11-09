"""Checks for registry_generator.py, which sits beside this file. Every rail and source here is fake.

Run: python3 -m unittest discover -s tools/registry
"""

import contextlib
import hashlib
import io
import json
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import registry_generator as rg  # noqa: E402

FAKE_RAIL = '''
import registry_generator as rg


class FakeRail(rg.RailHook):
    id = "fakechain"
    label = "Fakechain"
    networks = ("fake",)

    def parse_offer(self, lines, i, offer):
        if not lines[i].startswith("offer "):
            return None
        network, asset, amount = lines[i].split()[1:]
        offer.update(network=network, asset=asset,
                     payment={"scheme": "exact", "amount": amount, "pay_to": "fake-payee"})
        return i + 1


PLUGIN = FakeRail()
'''

# Entries are "<name>.txt" files in captures/. Line 1 is the name, later lines are rail constructs.
FAKE_SOURCE = '''
import registry_generator as rg


class FakeSource(rg.Source):
    id = "fake-captures"
    rails = ("fakechain",)
    location = "captures/<name>.txt"

    def discover(self, ctx):
        return sorted(p.relative_to(ctx.root).as_posix() for p in (ctx.root / "captures").glob("*.txt"))

    def parse(self, key, ctx):
        lines = (ctx.root / key).read_text(encoding="utf-8").splitlines()
        if not lines or not lines[0].startswith("name: "):
            raise rg.ParseError("missing name")
        offer = {"network": None, "asset": None, "payment": None}
        rail = ctx.rail(self, "Fakechain")
        i = 1
        while i < len(lines):
            nxt = rail.parse_offer(lines, i, offer)
            if nxt is None:
                raise rg.ParseError(f"unrecognised line {lines[i]!r}")
            i = nxt
        name = lines[0][6:]
        return record(name, key, **offer)


def record(name, key, network=None, asset=None, payment=None):
    return {
        "id": f"fake/{name}", "provider": "fake", "provider_name": "Fake", "name": name,
        "description": None, "method": "GET", "method_note": None,
        "url": f"https://example.test/{name}", "listed_price": "0.01", "listed_currency": "USD",
        "price_note": None, "category": "test", "parameters": None, "example_input": None,
        "inputs_incomplete": False, "parameters_partial": False, "parameters_unlisted": False,
        "request_reference": None, "status": "ok", "status_detail": "Fake.",
        "latest_check": {"method": "GET", "http_status": 402, "at": "2026-01-01T00:00:00Z"},
        "redirect_target": None, "quote_check_url": None, "network": network, "asset": asset,
        "payment": payment, "offer_issues": [], "provider_reference": None, "source_path": key,
    }


PLUGIN = FakeSource()
'''

CONFIG = """
# fake marketplace
output     out/registry.json
generator  make-registry
skips      skips.json
rail       plugins/fake_rail.py
source     plugins/fake_source.py
"""


class Workspace:
    """A temporary marketplace root with the fake plugins."""

    def __init__(self, config=CONFIG, captures=None, skips=None):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.write("plugins/fake_rail.py", FAKE_RAIL)
        self.write("plugins/fake_source.py", FAKE_SOURCE)
        self.write("skips.json", json.dumps(skips or {}))
        self.write("registry.conf", config)
        if captures is None:
            captures = {"b": "name: b\noffer fake:main TOKEN 100\n", "a": "name: a\n"}
        for name, text in captures.items():
            self.write(f"captures/{name}.txt", text)
        (self.root / "out").mkdir()

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def config(self):
        return rg.load_config(self.root / "registry.conf", self.root)

    def build(self):
        return rg.build(self.config())

    def close(self):
        self.tmp.cleanup()


class CoreTest(unittest.TestCase):
    def workspace(self, **kwargs):
        ws = Workspace(**kwargs)
        self.addCleanup(ws.close)
        return ws

    def test_builds_sorted_validated_records_from_a_fake_source_and_rail(self):
        registry, errors = self.workspace().build()
        self.assertEqual(errors, [])
        self.assertEqual(list(registry), ["schema_version", "source", "generator", "resource_count", "skipped", "resources"])
        self.assertEqual(registry["source"], "captures/<name>.txt")
        self.assertEqual(registry["generator"], "make-registry")
        self.assertEqual([r["id"] for r in registry["resources"]], ["fake/a", "fake/b"])
        self.assertEqual(list(registry["resources"][0]), list(rg.SCHEMA))
        b = registry["resources"][1]
        self.assertEqual((b["network"], b["asset"], b["payment"]["amount"]), ("fake:main", "TOKEN", "100"))

    def test_generation_is_deterministic_and_check_detects_staleness(self):
        ws = self.workspace()
        argv = ["--config", str(ws.root / "registry.conf"), "--root", str(ws.root)]
        quiet = io.StringIO()
        self.enterContext(contextlib.redirect_stdout(quiet))
        self.enterContext(contextlib.redirect_stderr(quiet))
        self.assertEqual(rg.main(argv + ["--check"]), 1)
        self.assertEqual(rg.main(argv), 0)
        first = (ws.root / "out/registry.json").read_bytes()
        self.assertEqual(rg.main(argv), 0)
        self.assertEqual((ws.root / "out/registry.json").read_bytes(), first)
        self.assertEqual(rg.main(argv + ["--check"]), 0)

    def test_unparsed_entries_need_a_skip_reason(self):
        captures = {"a": "name: a\n", "bad": "garbage\n"}
        registry, errors = self.workspace(captures=captures).build()
        self.assertEqual(errors, ["captures/bad.txt: missing name"])
        registry, errors = self.workspace(captures=captures, skips={"captures/bad.txt": "fixture"}).build()
        self.assertEqual(errors, [])
        self.assertEqual(registry["skipped"], [{"source_path": "captures/bad.txt", "reason": "fixture"}])
        _, errors = self.workspace(skips={"captures/a.txt": "x", "captures/gone.txt": "y"}).build()
        self.assertEqual(errors, [
            "captures/gone.txt: listed in skips.json but does not exist",
            "captures/a.txt: parses but is listed in skips.json",
        ])

    def test_rail_hook_rejects_unknown_lines_through_the_source(self):
        _, errors = self.workspace(captures={"a": "name: a\nsomething else\n"}).build()
        self.assertEqual(errors, ["captures/a.txt: unrecognised line 'something else'"])

    def test_schema_rejects_networks_outside_the_source_rails(self):
        _, errors = self.workspace(captures={"a": "name: a\noffer other:main TOKEN 1\n"}).build()
        self.assertEqual(errors, ["captures/a.txt: network 'other:main' is not covered by the source rails ['fakechain']"])

    def test_schema_rejects_bad_records(self):
        ws = self.workspace()
        config = ws.config()
        source = config.sources[0]
        ctx = rg.Context(ws.root, {r.id: r for r in config.rails})
        good = sys.modules["registry_plugin_plugins_fake_source_py"].record("a", "k")
        self.assertEqual(rg.validate(good, "k", source, ctx), [])
        cases = {
            "missing field": ({k: v for k, v in good.items() if k != "url"}, "fields missing ['url']"),
            "wrong type": ({**good, "inputs_incomplete": "no"}, "invalid inputs_incomplete"),
            "bad price": ({**good, "listed_price": "$1"}, "invalid listed_price"),
            "partial offer": ({**good, "network": "fake:main"}, "all be set or all be null"),
            "payment without payee": ({**good, "network": "fake:main", "asset": "T", "payment": {"scheme": "exact", "amount": "1"}}, "invalid payment"),
            "key mismatch": ({**good, "source_path": "other"}, "differs from the entry key"),
        }
        for name, (rec, expected) in cases.items():
            with self.subTest(name):
                problems = rg.validate(rec, "k", source, ctx)
                self.assertTrue(any(expected in p for p in problems), problems)

    def test_duplicate_ids_fail(self):
        ws = self.workspace(captures={"a": "name: same\n", "b": "name: same\n"})
        _, errors = ws.build()
        self.assertEqual(errors, ["captures/b.txt: duplicate id 'fake/same'"])

    def test_configuration_errors(self):
        cases = {
            "unknown entry": CONFIG + "plugin x.py\n",
            "missing output": CONFIG.replace("output     out/registry.json\n", ""),
            "missing plugin file": CONFIG + "source plugins/missing.py\n",
            "duplicate source": CONFIG + "source plugins/fake_source.py\n",
            "unregistered rail": CONFIG.replace("rail       plugins/fake_rail.py\n", ""),
            "rail registered as source": CONFIG + "source plugins/fake_rail.py\n",
            "url without digest": CONFIG + "registry https://example.test/registry.json\n",
            "unknown registry option": CONFIG + "registry vendor.json pin=1\n",
        }
        for name, config in cases.items():
            with self.subTest(name), self.assertRaises(rg.ConfigError):
                self.workspace(config=config).config()

    def test_delegated_module_source_loads_by_module_name(self):
        """A vendor package registers a source by importable module name instead of a file path."""
        ws = self.workspace(config=CONFIG.replace("plugins/fake_source.py", "vendor_pkg.registry_source"))
        ws.write("site/vendor_pkg/__init__.py", "")
        ws.write("site/vendor_pkg/registry_source.py", FAKE_SOURCE.replace('id = "fake-captures"', 'id = "vendor-owned"'))
        sys.path.insert(0, str(ws.root / "site"))
        self.addCleanup(sys.path.remove, str(ws.root / "site"))
        self.addCleanup(lambda: [sys.modules.pop(m, None) for m in ("vendor_pkg", "vendor_pkg.registry_source")])
        config = ws.config()
        self.assertEqual([s.id for s in config.sources], ["vendor-owned"])
        registry, errors = rg.build(config)
        self.assertEqual(errors, [])
        self.assertEqual(registry["resource_count"], 2)

    def test_delegated_published_registry_merges_with_maintained_sources(self):
        ws = self.workspace()
        own, _ = ws.build()
        published = dict(own, resources=[dict(r, id="vendor/" + r["name"], source_path="vendor/" + r["name"])
                                         for r in own["resources"]],
                         skipped=[{"source_path": "vendor/z", "reason": "vendor skip"}])
        data = rg.render(published).encode()
        ws.write("vendor-registry.json", data.decode())
        digest = hashlib.sha256(data).hexdigest()
        url = (ws.root / "vendor-registry.json").as_uri()
        for line in ("registry vendor-registry.json", f"registry {url} rails=fakechain sha256={digest}"):
            with self.subTest(line):
                ws.write("registry.conf", CONFIG + line + "\n")
                registry, errors = ws.build()
                self.assertEqual(errors, [])
                self.assertEqual(registry["source"], f"captures/<name>.txt; {line.split()[1]}")
                self.assertEqual([r["source_path"] for r in registry["resources"]],
                                 ["captures/a.txt", "captures/b.txt", "vendor/a", "vendor/b"])
                self.assertEqual(registry["skipped"], [{"source_path": "vendor/z", "reason": "vendor skip"}])
        ws.write("registry.conf", CONFIG + f"registry {url} sha256={'0' * 64}\n")
        with self.assertRaises(rg.ConfigError):
            ws.build()

    def test_text_helpers(self):
        lines = textwrap.dedent("""\
            | Field | Value |
            | --- | --- |
            | a | `x` |
            | b \\| c | [t](u) |
            after""").splitlines()
        rows, nxt = rg.read_table(lines, 0, ["Field", "Value"])
        self.assertEqual(rows, [["a", "`x`"], ["b | c", "[t](u)"]])
        self.assertEqual(lines[nxt], "after")
        self.assertEqual(rg.code(rows[0][1]), "x")
        self.assertEqual(rg.link(rows[1][1]), ("t", "u"))
        for bad in (lambda: rg.read_table(lines, 1, ["Field", "Value"]), lambda: rg.code("x"),
                    lambda: rg.link("t"), lambda: rg.section(lines, "missing", None)):
            with self.assertRaises(rg.ParseError):
                bad()


if __name__ == "__main__":
    unittest.main()
