#!/usr/bin/env python3
"""Re-align markdown table columns. Skips fenced code blocks."""

from __future__ import annotations

import re
import sys


def is_table_line(line: str) -> bool:
    return re.match(r"^\s*\|", line) is not None


def parse_cells(line: str) -> list[str]:
    s = line.rstrip("\n").strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|"):
        s = s[:-1]
    return [c.strip() for c in s.split("|")]


def is_separator_line(line: str) -> bool:
    if not is_table_line(line):
        return False
    cells = parse_cells(line)
    if not cells:
        return False
    return all(re.match(r"^:?-{3,}:?$", c.strip()) for c in cells)


def parse_alignment(sep_cell: str) -> str | None:
    c = sep_cell.strip()
    if c.startswith(":") and c.endswith(":"):
        return "center"
    if c.endswith(":"):
        return "right"
    if c.startswith(":"):
        return "left"
    return None


def format_table(rows: list[tuple[int, list[str]]]) -> list[str] | None:
    if len(rows) < 2:
        return None
    n_cols = len(rows[0][1])
    sep_cells = rows[1][1]
    if n_cols != len(sep_cells):
        return None
    aligns = [parse_alignment(c) for c in sep_cells]

    widths = [0] * n_cols
    for _, cells in rows:
        for i in range(n_cols):
            cell = cells[i] if i < len(cells) else ""
            widths[i] = max(widths[i], len(cell))
    widths = [max(w, 3) for w in widths]

    out: list[str] = []
    for idx, (_, cells) in enumerate(rows):
        new_cells: list[str] = []
        for i in range(n_cols):
            cell = cells[i] if i < len(cells) else ""
            w = widths[i]
            if idx == 1:
                a = aligns[i]
                if a == "center":
                    new_cells.append(":" + "-" * (w - 2) + ":")
                elif a == "right":
                    new_cells.append("-" * (w - 1) + ":")
                elif a == "left":
                    new_cells.append(":" + "-" * (w - 1))
                else:
                    new_cells.append("-" * w)
            else:
                a = aligns[i]
                if a == "right":
                    new_cells.append(cell.rjust(w))
                elif a == "center":
                    new_cells.append(cell.center(w))
                else:
                    new_cells.append(cell.ljust(w))
        out.append("| " + " | ".join(new_cells) + " |")
    return out


def process_file(path: str) -> int:
    with open(path) as f:
        lines = f.readlines()

    output: list[str] = []
    i = 0
    in_code = False
    tables_reformatted = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.rstrip("\n")

        if re.match(r"^\s*```", stripped):
            in_code = not in_code
            output.append(line)
            i += 1
            continue

        if in_code:
            output.append(line)
            i += 1
            continue

        if (
            is_table_line(stripped)
            and i + 1 < len(lines)
            and is_separator_line(lines[i + 1].rstrip("\n"))
        ):
            j = i
            rows: list[tuple[int, list[str]]] = []
            while j < len(lines) and is_table_line(lines[j].rstrip("\n")):
                rows.append((j, parse_cells(lines[j])))
                j += 1
            formatted = format_table(rows)
            if formatted is not None:
                tables_reformatted += 1
                for fline in formatted:
                    output.append(fline + "\n")
                i = j
                continue

        output.append(line)
        i += 1

    with open(path, "w") as f:
        f.writelines(output)
    return tables_reformatted


if __name__ == "__main__":
    for p in sys.argv[1:]:
        n = process_file(p)
        print(f"{p}: re-aligned {n} table(s)")
