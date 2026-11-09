#!/usr/bin/env python3
"""Generate a marketplace registry JSON file from registered source plugins.

Usage:
    python3 registry_generator.py --config <file> --root <dir>          write the registry
    python3 registry_generator.py --config <file> --root <dir> --check  fail if it is stale

Standard library only. The core names no rail and no marketplace. A marketplace
repository supplies a configuration file and its plugins. The core loads the
registered rail hooks and sources, collects one record per source entry, validates
each record against the registry schema and writes the file deterministically.

Configuration: one setting or registration per line, relative to --root. '#' starts a comment.

    output     registry.json                  generated file
    generator  scripts/generate_registry.py   value of the registry "generator" field
    skips      scripts/registry_skip.json     optional {"<entry key>": "<reason>"}
    rail       plugins/example_rail.py        rail hook: a .py file or an importable module
    source     plugins/example_source.py      source plugin: a .py file or an importable module
    registry   <file or URL> [rails=<id>,...] [sha256=<hex>]
               a registry file that a vendor publishes; a URL needs sha256

A plugin module assigns one RailHook or Source object to PLUGIN.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import importlib.util
import json
import re
import sys
import urllib.request
from pathlib import Path

SCHEMA_VERSION = 1
PARAM_LOCATIONS = ("query", "body", "path", "header")
PAYMENT_REQUIRED = ("scheme", "amount", "pay_to")
# Key order of nested objects in the output. Input key order does not matter.
PARAMETER_KEYS = ("name", "in", "type", "required")
LATEST_CHECK_KEYS = ("method", "http_status", "at")
SKIP_KEYS = ("source_path", "reason")


class ParseError(Exception):
    """An entry does not parse. The skip file may list it with a reason."""


class ConfigError(Exception):
    """The configuration or a plugin registration is invalid."""


# Plugin interfaces ---------------------------------------------------------------------------


class RailHook:
    """Rail-specific payment offer parsing.

    id        stable id that a source names in its `rails`
    label     rail name as captures spell it
    networks  CAIP-2 network ids or namespaces; a namespace "ns" covers every "ns:<ref>"
    """

    id = ""
    label = ""
    networks: tuple[str, ...] = ()

    def parse_offer(self, lines: list[str], i: int, offer: dict) -> int | None:
        """Read the rail construct at lines[i] into offer {network, asset, payment}.

        Return the index after the construct, or None if lines[i] is not a construct of this
        rail. Raise ParseError for a malformed construct.
        """
        raise NotImplementedError


def covers(rail, network: str) -> bool:
    """Whether a rail hook's networks include this CAIP-2 network id."""
    return any(network == n or network.startswith(n + ":") for n in rail.networks)


class Source:
    """A marketplace source of registry records.

    id        stable source id
    rails     ids of the rail hooks whose offers the source lists
    location  where the entries come from; recorded in the registry "source" field
    skipped   entries the source excluded itself, as {"source_path", "reason"}
    """

    id = ""
    rails: tuple[str, ...] = ()
    location = ""
    skipped: tuple[dict, ...] = ()

    def discover(self, ctx: "Context") -> list[str]:
        """Return the entry keys. A key becomes the record's source_path."""
        raise NotImplementedError

    def parse(self, key: str, ctx: "Context") -> dict:
        """Return the record for one entry. Raise ParseError if the entry does not parse."""
        raise NotImplementedError


class Context:
    """What the core gives a source: the root and the registered rail hooks."""

    def __init__(self, root: Path, rails: dict[str, RailHook]):
        self.root = root
        self.rails = rails

    def rail(self, source: Source, label: str) -> RailHook:
        """Return the rail hook with this capture label among the source's rails."""
        for rail_id in source.rails:
            if self.rails[rail_id].label == label:
                return self.rails[rail_id]
        raise ParseError(f"no registered rail hook for {label!r}")


# Text helpers for capture formats -------------------------------------------------------------


def find(lines: list[str], item: str, start: int = 0) -> int:
    """Return the index of `item`, or raise ParseError."""
    try:
        return lines.index(item, start)
    except ValueError:
        raise ParseError(f"missing {item!r}") from None


def split_row(line: str) -> list[str]:
    if not (line.startswith("|") and line.endswith("|")):
        raise ParseError(f"not a table row: {line!r}")
    cells = re.split(r"(?<!\\)\|", line[1:-1])
    return [c.strip().replace("\\|", "|") for c in cells]


def read_table(lines: list[str], i: int, header: list[str]) -> tuple[list[list[str]], int]:
    """Read a Markdown table whose header row is lines[i]. Return rows and the next index."""
    if split_row(lines[i]) != header:
        raise ParseError(f"unexpected table header {lines[i]!r}")
    if i + 1 >= len(lines) or not re.fullmatch(r"\|( -+:? \|)+", lines[i + 1]):
        raise ParseError(f"missing table separator after {lines[i]!r}")
    rows = []
    i += 2
    while i < len(lines) and lines[i].startswith("|"):
        row = split_row(lines[i])
        if len(row) != len(header):
            raise ParseError(f"wrong cell count in {lines[i]!r}")
        rows.append(row)
        i += 1
    return rows, i


def link(cell: str) -> tuple[str, str]:
    m = re.match(r"^\[([^\]]+)\]\(([^)]+)\)$", cell)
    if not m:
        raise ParseError(f"expected a Markdown link, got {cell!r}")
    return m.group(1), m.group(2)


def code(cell: str) -> str:
    if not (len(cell) >= 2 and cell.startswith("`") and cell.endswith("`")):
        raise ParseError(f"expected a code span, got {cell!r}")
    return cell[1:-1]


def section(lines: list[str], start: str, end: str | None) -> list[str]:
    """Return the lines between the line `start` and the line `end` (or the end of text)."""
    a = find(lines, start)
    b = find(lines, end) if end else len(lines)
    if b < a:
        raise ParseError(f"section {end!r} precedes {start!r}")
    return lines[a + 1 : b]


# Registry schema ------------------------------------------------------------------------------


def _str(v) -> bool:
    return isinstance(v, str)


def _opt_str(v) -> bool:
    return v is None or isinstance(v, str)


def _bool(v) -> bool:
    return isinstance(v, bool)


def _parameters(v) -> bool:
    return v is None or (isinstance(v, list) and all(
        isinstance(p, dict) and set(p) == set(PARAMETER_KEYS)
        and _str(p["name"]) and p["in"] in PARAM_LOCATIONS and _opt_str(p["type"]) and _bool(p["required"])
        for p in v
    ))


def _latest_check(v) -> bool:
    return (isinstance(v, dict) and set(v) == set(LATEST_CHECK_KEYS) and _str(v["method"])
            and isinstance(v["http_status"], int) and not _bool(v["http_status"]) and _str(v["at"]))


def _payment(v) -> bool:
    return v is None or (isinstance(v, dict) and all(_str(v.get(k)) and v[k] for k in PAYMENT_REQUIRED)
                         and all(x is None or isinstance(x, (str, bool, int, float)) for x in v.values()))


# Field order is the output order.
SCHEMA = {
    "id": _str,
    "provider": _str,
    "provider_name": _str,
    "name": _str,
    "description": _opt_str,
    "method": _str,
    "method_note": _opt_str,
    "url": _str,
    "listed_price": lambda v: _str(v) and re.fullmatch(r"\d+(\.\d+)?", v) is not None,
    "listed_currency": _str,
    "price_note": _opt_str,
    "category": _str,
    "parameters": _parameters,
    "example_input": lambda v: True,
    "inputs_incomplete": _bool,
    "parameters_partial": _bool,
    "parameters_unlisted": _bool,
    "request_reference": _opt_str,
    "status": _str,
    "status_detail": _str,
    "latest_check": _latest_check,
    "redirect_target": _opt_str,
    "quote_check_url": _opt_str,
    "network": _opt_str,
    "asset": _opt_str,
    "payment": _payment,
    "offer_issues": lambda v: isinstance(v, list) and all(_str(x) for x in v),
    "provider_reference": _opt_str,
    "source_path": _str,
}


def ordered(rec: dict) -> dict:
    """Return a valid record with schema field order and canonical nested key order."""
    out = {name: rec[name] for name in SCHEMA}
    out["latest_check"] = {k: rec["latest_check"][k] for k in LATEST_CHECK_KEYS}
    if rec["parameters"] is not None:
        out["parameters"] = [{k: p[k] for k in PARAMETER_KEYS} for p in rec["parameters"]]
    return out


def validate(rec: dict, key: str, source: Source, ctx: Context) -> list[str]:
    """Return schema problems in one record."""
    if not isinstance(rec, dict):
        return [f"record is {type(rec).__name__}, not an object"]
    missing, extra = sorted(set(SCHEMA) - set(rec)), sorted(set(rec) - set(SCHEMA))
    if missing or extra:
        return [f"fields missing {missing}, unexpected {extra}"]
    problems = [f"invalid {name}: {rec[name]!r}" for name, ok in SCHEMA.items() if not ok(rec[name])]
    if rec["source_path"] != key:
        problems.append(f"source_path {rec['source_path']!r} differs from the entry key")
    offer = [rec["network"] is None, rec["asset"] is None, rec["payment"] is None]
    if any(offer) and not all(offer):
        problems.append("network, asset and payment must all be set or all be null")
    elif rec["network"] is not None and not any(covers(ctx.rails[r], rec["network"]) for r in source.rails):
        problems.append(f"network {rec['network']!r} is not covered by the source rails {list(source.rails)}")
    return problems


# Delegated published registries ---------------------------------------------------------------


class PublishedRegistry(Source):
    """Records from a registry file that a vendor publishes in this schema."""

    def __init__(self, location: str, rails: tuple[str, ...] | None, sha256: str | None):
        if "://" in location and not sha256:
            raise ConfigError(f"registry {location}: a URL needs sha256=<hex>")
        self.id = f"registry:{location}"
        self.location = location
        self.rails = rails
        self.sha256 = sha256
        self.records: dict[str, dict] = {}
        self.skipped = []

    def discover(self, ctx: Context) -> list[str]:
        if "://" in self.location:
            with urllib.request.urlopen(self.location, timeout=30) as response:
                data = response.read()
        else:
            data = (ctx.root / self.location).read_bytes()
        if self.sha256 and hashlib.sha256(data).hexdigest() != self.sha256.lower():
            raise ConfigError(f"registry {self.location}: sha256 does not match")
        registry = json.loads(data)
        if not isinstance(registry, dict) or registry.get("schema_version") != SCHEMA_VERSION:
            raise ConfigError(f"registry {self.location}: schema_version is not {SCHEMA_VERSION}")
        try:
            self.records = {r["source_path"]: r for r in registry["resources"]}
        except (KeyError, TypeError):
            raise ConfigError(f"registry {self.location}: resources need source_path") from None
        if len(self.records) != len(registry["resources"]):
            raise ConfigError(f"registry {self.location}: duplicate source_path")
        self.skipped = list(registry.get("skipped", []))
        return list(self.records)

    def parse(self, key: str, ctx: Context) -> dict:
        return dict(self.records[key])


# Configuration --------------------------------------------------------------------------------


class Config:
    def __init__(self, root: Path, output: str, generator: str, skips: str | None,
                 rails: list[RailHook], sources: list[Source]):
        self.root = root
        self.output = root / output
        self.generator = generator
        self.skips = root / skips if skips else None
        self.rails = rails
        self.sources = sources


def load_plugin(spec: str, root: Path):
    """Load PLUGIN from a .py file under root or from an importable module."""
    if spec.endswith(".py"):
        path = root / spec
        if not path.is_file():
            raise ConfigError(f"plugin file {spec} does not exist")
        name = "registry_plugin_" + re.sub(r"\W", "_", spec)
        module_spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(module_spec)
        sys.modules[name] = module
        module_spec.loader.exec_module(module)
    else:
        module = importlib.import_module(spec)
    if not hasattr(module, "PLUGIN"):
        raise ConfigError(f"plugin {spec} does not define PLUGIN")
    return module.PLUGIN


def _check_plugin(spec: str, plugin, kind: type) -> None:
    attrs = ("id", "label", "networks", "parse_offer") if kind is RailHook else ("id", "rails", "location", "discover", "parse")
    missing = [a for a in attrs if not hasattr(plugin, a)]
    if missing:
        raise ConfigError(f"{spec}: not a {kind.__name__}, missing {missing}")
    if not (isinstance(plugin.id, str) and plugin.id):
        raise ConfigError(f"{spec}: empty id")


def load_config(path: Path, root: Path) -> Config:
    settings: dict[str, str] = {}
    rails: list[RailHook] = []
    sources: list[Source] = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        words = raw.split("#", 1)[0].split()
        if not words:
            continue
        kind, args = words[0], words[1:]
        where = f"{path.name}:{number}"
        if kind in ("output", "generator", "skips"):
            if len(args) != 1 or kind in settings:
                raise ConfigError(f"{where}: {kind} takes one value, once")
            settings[kind] = args[0]
        elif kind in ("rail", "source"):
            if len(args) != 1:
                raise ConfigError(f"{where}: {kind} takes one module")
            plugin = load_plugin(args[0], root)
            _check_plugin(args[0], plugin, RailHook if kind == "rail" else Source)
            (rails if kind == "rail" else sources).append(plugin)
        elif kind == "registry":
            if not args:
                raise ConfigError(f"{where}: registry needs a location")
            options = dict(a.split("=", 1) for a in args[1:] if "=" in a)
            if len(options) != len(args) - 1 or set(options) - {"rails", "sha256"}:
                raise ConfigError(f"{where}: registry options are rails=<id>,... and sha256=<hex>")
            rail_ids = tuple(options["rails"].split(",")) if "rails" in options else None
            sources.append(PublishedRegistry(args[0], rail_ids, options.get("sha256")))
        else:
            raise ConfigError(f"{where}: unknown entry {kind!r}")
    for required in ("output", "generator"):
        if required not in settings:
            raise ConfigError(f"{path.name}: missing {required}")
    for plugins in (rails, sources):
        ids = [p.id for p in plugins]
        if len(ids) != len(set(ids)):
            raise ConfigError(f"{path.name}: duplicate plugin id in {ids}")
    rail_ids = {r.id for r in rails}
    for source in sources:
        if source.rails is None:  # a published registry without rails= may use every registered rail
            source.rails = tuple(r.id for r in rails)
        if set(source.rails) - rail_ids:
            raise ConfigError(f"source {source.id}: unregistered rails {sorted(set(source.rails) - rail_ids)}")
    if not sources:
        raise ConfigError(f"{path.name}: no source registered")
    return Config(root, settings["output"], settings["generator"], settings.get("skips"), rails, sources)


# Build ----------------------------------------------------------------------------------------


def load_skips(config: Config) -> dict[str, str]:
    if config.skips is None:
        return {}
    if not config.skips.is_file():
        raise ConfigError(f"skip file {config.skips} does not exist")
    data = json.loads(config.skips.read_text(encoding="utf-8"))
    for key, reason in data.items():
        if not isinstance(reason, str) or not reason.strip():
            raise ConfigError(f"{config.skips.name}: {key} needs a non-empty reason")
    return data


def build(config: Config) -> tuple[dict, list[str]]:
    """Return the registry and a list of errors (unlisted failures, stale skips, schema problems)."""
    skips = load_skips(config)
    skip_name = config.skips.name if config.skips else "skips"
    ctx = Context(config.root, {r.id: r for r in config.rails})
    skipped: list[dict] = []
    errors: list[str] = []
    owners: dict[str, Source] = {}  # entry keys to parse
    claimed: dict[str, Source] = {}  # every key: entries and the sources' own skipped entries

    def claim(key: str, source: Source) -> None:
        if key in claimed:
            errors.append(f"{key}: claimed by {claimed[key].id} and {source.id}")
        claimed[key] = source

    for source in config.sources:
        for key in source.discover(ctx):
            claim(key, source)
            owners.setdefault(key, source)
        for skip in getattr(source, "skipped", ()):
            if not (isinstance(skip, dict) and set(skip) == set(SKIP_KEYS)
                    and _str(skip["source_path"]) and _str(skip["reason"]) and skip["reason"].strip()):
                raise ConfigError(f"source {source.id}: invalid skipped entry {skip!r}")
            claim(skip["source_path"], source)
            skipped.append({k: skip[k] for k in SKIP_KEYS})
    for stale in sorted(set(skips) - set(owners)):
        errors.append(f"{stale}: listed in {skip_name} but does not exist")
    resources: list[dict] = []
    ids: set[str] = set()
    for key in sorted(owners):
        source = owners[key]
        try:
            rec = source.parse(key, ctx)
        except ParseError as exc:
            if key in skips:
                skipped.append({"source_path": key, "reason": skips[key]})
            else:
                errors.append(f"{key}: {exc}")
            continue
        if key in skips:
            errors.append(f"{key}: parses but is listed in {skip_name}")
        problems = validate(rec, key, source, ctx)
        if not problems and rec["id"] in ids:
            problems.append(f"duplicate id {rec['id']!r}")
        if problems:
            errors.extend(f"{key}: {p}" for p in problems)
            continue
        ids.add(rec["id"])
        resources.append(ordered(rec))
    registry = {
        "schema_version": SCHEMA_VERSION,
        "source": "; ".join(s.location for s in config.sources),
        "generator": config.generator,
        "resource_count": len(resources),
        "skipped": sorted(skipped, key=lambda s: s["source_path"]),
        "resources": resources,
    }
    return registry, errors


def render(registry: dict) -> str:
    return json.dumps(registry, indent=2, ensure_ascii=False) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--config", required=True, type=Path, help="configuration file")
    parser.add_argument("--root", required=True, type=Path, help="directory that configuration paths are relative to")
    parser.add_argument("--check", action="store_true", help="fail if the output file is stale")
    args = parser.parse_args(argv)
    try:
        config = load_config(args.config, args.root.resolve())
        registry, errors = build(config)
    except ConfigError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    for err in errors:
        print(f"error: {err}", file=sys.stderr)
    if errors:
        return 1
    text = render(registry)
    name = config.output.name
    total = registry["resource_count"] + len(registry["skipped"])
    summary = f"{registry['resource_count']}/{total} entries parsed, {len(registry['skipped'])} skipped"
    if args.check:
        current = config.output.read_text(encoding="utf-8") if config.output.exists() else ""
        if current != text:
            print(f"error: {name} is stale; run {config.generator}", file=sys.stderr)
            return 1
        print(f"{name} is current: {summary}")
        return 0
    config.output.write_text(text, encoding="utf-8")
    print(f"wrote {name}: {summary}")
    return 0


if __name__ == "__main__":
    # Plugins import this module by name, so run the imported copy, not __main__.
    import registry_generator

    sys.exit(registry_generator.main())
