# Registry generator

`registry_generator.py` builds a marketplace registry JSON file from registered plugins.
It uses the Python standard library only and names no rail, vendor or marketplace.
A marketplace repository keeps its catalog data, its plugins and one configuration file.
It pins this directory by Git revision and runs the generator with its own configuration.

```sh
python3 tools/registry/registry_generator.py --config <file> --root <dir>          # write
python3 tools/registry/registry_generator.py --config <file> --root <dir> --check  # fail if stale
python3 -m unittest discover -s tools/registry                                      # tests
```

## Plugins

A plugin module assigns one object to `PLUGIN`. The configuration registers each plugin on one line.

- **Source** (`Source`): declares `id`, `rails` and `location`.
  `discover(ctx)` returns entry keys. `parse(key, ctx)` returns one record, or raises `ParseError`.
  A source maintained by the marketplace is a `.py` file in its repository.
  A source maintained by a vendor is an importable module from the vendor's package.
- **Published registry**: a `registry <file or URL>` line loads a registry file that a vendor publishes in this schema.
  A URL needs `sha256=<hex>`. `rails=<id>,...` names the rails that its records may use.
- **Rail hook** (`RailHook`): declares `id`, `label` and `networks` (CAIP-2 ids or namespaces).
  `parse_offer(lines, i, offer)` reads one rail-specific payment offer construct.
  A source calls `ctx.rail(source, label)` and hands the hook the lines that it does not recognise.

```text
output     registry.json
generator  scripts/generate_registry.py
skips      scripts/registry_skip.json
rail       scripts/plugins/example_rail.py
source     scripts/plugins/example_source.py
source     example_vendor.registry_source
registry   https://example.com/registry.json rails=example sha256=<hex>
```

## What the core does

1. It loads the rail hooks and sources in configuration order and checks that each source names registered rails.
2. It collects entry keys from every source. A key that two sources discover is an error.
3. It parses each entry in key order. An entry that raises `ParseError` must appear in the skip file with a reason.
   A skip entry for a missing key, or for an entry that parses, is an error.
4. It validates each record against `SCHEMA`: field set, field types, an all-or-nothing payment offer,
   unique `id`, `source_path` equal to the entry key, and a `network` that one of the source's rail hooks covers.
5. It writes `schema_version`, `source` (the source locations joined by `; `), `generator`, `resource_count`,
   `skipped` and `resources`, sorted by `source_path`, as indented UTF-8 JSON.

The helpers `find`, `split_row`, `read_table`, `link`, `code` and `section` parse Markdown captures.
