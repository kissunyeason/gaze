#!/usr/bin/env python3
"""Apply the Simplified Chinese string table to the Rust sources.

The table lives in ``localization/zh-CN/*.json`` and maps the exact English
literal found in the sources to its Chinese replacement.  Only real string
literals (and ``///`` doc comments, for clap help texts) are rewritten, never
comments or raw strings, and the pass is idempotent: once a literal is
translated the English key no longer matches.

Usage:
    scripts/localize-zh.py            # rewrite sources in place
    scripts/localize-zh.py --check    # report untranslated user-facing strings
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLE_DIR = ROOT / "localization" / "zh-CN"
SOURCE_DIRS = ["crates"]


def load_table() -> list[tuple[tuple[str, ...], dict[str, str]]]:
    """Load the scoped tables.

    Every table file carries a ``_scope`` list of glob patterns, so an entry
    such as ``Config`` can rename a GUI tab without ever touching the D-Bus
    interface of the same name in another crate.
    """
    tables = []
    for path in sorted(TABLE_DIR.glob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        scope = tuple(data.pop("_scope", ("crates/**",)))
        entries = {key: value for key, value in data.items() if not key.startswith("_")}
        tables.append((scope, entries))
    return tables


def matches(relpath: str, pattern: str) -> bool:
    if pattern.endswith("/**"):
        prefix = pattern[:-3]
        return relpath == prefix or relpath.startswith(prefix + "/")
    return pathlib.PurePath(relpath).match(pattern)


def table_for(relpath: str, tables) -> dict[str, str]:
    merged: dict[str, str] = {}
    for scope, entries in tables:
        if any(matches(relpath, pattern) for pattern in scope):
            merged.update(entries)
    return merged


def scan_literals(src: str):
    """Yield (start, end, content) for regular "..." literals.

    Raw strings (r"..", r#".."#), block/line comments and char literals are
    skipped, so a translation can never corrupt anything but a real literal.
    """
    i, n = 0, len(src)
    while i < n:
        ch = src[i]
        if ch == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif ch == "/" and i + 1 < n and src[i + 1] == "*":
            depth, i = 1, i + 2
            while i < n and depth:
                if src.startswith("/*", i):
                    depth += 1
                    i += 2
                elif src.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
        elif ch == "r" and i + 1 < n and src[i + 1] in '"#':
            match = re.match(r'r(#*)"', src[i:])
            if not match:
                i += 1
                continue
            end_marker = '"' + match.group(1)
            start = i + len(match.group(0))
            end = src.find(end_marker, start)
            i = n if end < 0 else end + len(end_marker)
        elif ch == "b" and i + 1 < n and src[i + 1] == '"':
            i += 1
        elif ch == '"':
            j = i + 1
            buf = []
            while j < n:
                if src[j] == "\\" and j + 1 < n:
                    buf.append(src[j : j + 2])
                    j += 2
                    continue
                if src[j] == '"':
                    break
                buf.append(src[j])
                j += 1
            yield i, j + 1, "".join(buf)
            i = j + 1
        elif ch == "'":
            match = re.match(r"'(?:[^'\\]|\\.)'", src[i:])
            i = i + len(match.group(0)) if match else i + 1
        else:
            i += 1


def normalize(text: str) -> str:
    """Collapse line breaks and indentation so multi-line literals match."""
    return re.sub(r"\s+", " ", text.replace("\n", " ")).strip()


def user_facing(text: str) -> bool:
    if not re.search(r"[A-Za-z]{3,}", text):
        return False
    if re.fullmatch(r"[a-z0-9_.:/=\-]+", text):
        return False
    if text.startswith(("com.gundulabs", "gaze@", "org.gnome", "/usr/", "/etc/")):
        return False
    return True


def apply(path: pathlib.Path, table: dict[str, str], report: dict[str, int]) -> bool:
    src = path.read_text(encoding="utf-8")
    edits: list[tuple[int, int, str]] = []

    for start, end, content in scan_literals(src):
        zh = table.get(content) or table.get(normalize(content))
        if zh is not None and zh != content:
            edits.append((start, end, json.dumps(zh, ensure_ascii=False)))
            report[content] = report.get(content, 0) + 1

    for key, zh in table.items():
        if not key.startswith("/// "):
            continue
        english = key[4:]
        zh_text = zh[4:] if zh.startswith("/// ") else zh
        pattern = re.compile(
            r"^([ \t]*)/// " + r"\s+".join(re.escape(part) for part in english.split(" ")) + r"[ \t]*$",
            re.MULTILINE,
        )
        for match in pattern.finditer(src):
            line_start = match.start()
            line_end = match.end()
            edits.append((line_start, line_end, f"{match.group(1)}/// {zh_text}"))
            report[key] = report.get(key, 0) + 1

    if not edits:
        return False

    out, cursor = [], 0
    for start, end, text in sorted(edits):
        if start < cursor:
            continue
        out.append(src[cursor:start])
        out.append(text)
        cursor = end
    out.append(src[cursor:])
    path.write_text("".join(out), encoding="utf-8")
    return True


def check(tables) -> None:
    total = 0
    for directory in SOURCE_DIRS:
        for path in sorted((ROOT / directory).rglob("*.rs")):
            table = table_for(str(path.relative_to(ROOT)), tables)
            if not table:
                continue
            src = path.read_text(encoding="utf-8")
            missing = sorted(
                {
                    normalize(content)
                    for _, _, content in scan_literals(src)
                    if (
                        content.isascii()
                        and user_facing(content)
                        and content not in table
                        and normalize(content) not in table
                    )
                }
            )
            if missing:
                total += len(missing)
                print(f"\n{path.relative_to(ROOT)} ({len(missing)})")
                for text in missing:
                    print(f"  {text}")
    print(f"\n未翻译的候选字符串：{total}")


def main() -> int:
    tables = load_table()
    if "--check" in sys.argv:
        check(tables)
        return 0

    report: dict[str, int] = {}
    changed = 0
    for directory in SOURCE_DIRS:
        for path in sorted((ROOT / directory).rglob("*.rs")):
            table = table_for(str(path.relative_to(ROOT)), tables)
            if table and apply(path, table, report):
                changed += 1

    total = sum(len(entries) for _, entries in tables)
    print(f"已改写 {changed} 个文件，本次命中 {len(report)}/{total} 条词表条目")
    return 0


if __name__ == "__main__":
    sys.exit(main())
