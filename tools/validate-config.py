#!/usr/bin/env python3
"""Validate .coderef.jsonc files against schema/coderef.schema.json.

Strips JSONC comments (// line, /* block */) and trailing commas before
validating with `jsonschema` (draft 2020-12). Exits 0 on success, 1 on
any validation failure.

Usage:
    python3 tools/validate-config.py <schema.json> <config.jsonc> [<config.jsonc> ...]
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

try:
    import jsonschema
except ImportError:
    sys.stderr.write(
        "validate-config: jsonschema not installed. "
        "Try `python3 -m pip install --user jsonschema`.\n"
    )
    sys.exit(1)


_LINE_COMMENT = re.compile(r"^\s*//.*$", re.MULTILINE)
# Block-comment removal. Naive — does not respect `//` or `/*` inside strings.
# Fine for our hand-authored example configs; not robust against adversarial
# JSONC input.
_BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.DOTALL)
_TRAILING_COMMA = re.compile(r",(\s*[}\]])")


def strip_jsonc(src: str) -> str:
    src = _LINE_COMMENT.sub("", src)
    src = _BLOCK_COMMENT.sub("", src)
    src = _TRAILING_COMMA.sub(r"\1", src)
    return src


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        sys.stderr.write(__doc__ or "")
        return 1
    schema_path = Path(argv[1])
    schema = json.loads(schema_path.read_text())
    failed = 0
    for cfg_arg in argv[2:]:
        cfg_path = Path(cfg_arg)
        raw = cfg_path.read_text()
        stripped = strip_jsonc(raw)
        try:
            config = json.loads(stripped)
        except json.JSONDecodeError as exc:
            sys.stderr.write(f"{cfg_path}: JSON parse error after stripping JSONC: {exc}\n")
            failed += 1
            continue
        try:
            jsonschema.validate(config, schema)
        except jsonschema.ValidationError as exc:
            sys.stderr.write(f"{cfg_path}: validation error\n  {exc.message}\n  at: {list(exc.absolute_path)}\n")
            failed += 1
            continue
        print(f"{cfg_path}: ok")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
