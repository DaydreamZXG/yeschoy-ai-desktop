#!/usr/bin/env python3
"""YesChoy desktop-connect installer helpers. Marker edits for New API source trees."""
from __future__ import annotations

import re
import sys
from pathlib import Path

BEGIN = "// >>> YESCHOY_DESKTOP_CONNECT_BEGIN"
END = "// <<< YESCHOY_DESKTOP_CONNECT_END"


def _write(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")


def patch_api_router(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    notes: list[str] = []
    changed = False

    if "DesktopBearerAuth" not in text:
        m = re.search(r"(apiRouter\.Use\([^\n]*\n)", text)
        if not m:
            notes.append("WARN:no-apiRouter.Use")
        else:
            block = (
                f"{m.group(1)}"
                f"\t{BEGIN}\n"
                f"\tapiRouter.Use(middleware.DesktopBearerAuth())\n"
                f"\t{END}\n"
            )
            text = text[: m.start()] + block + text[m.end() :]
            changed = True
            notes.append("ok:DesktopBearerAuth")
    else:
        notes.append("skip:DesktopBearerAuth")

    if "registerDesktopConnectRoutes" not in text:
        m = re.search(r'apiRouter\s*:?=\s*router\.Group\("/api"\)', text)
        if not m:
            notes.append("WARN:no-apiRouter-Group")
        else:
            rest = text[m.end() :]
            lines = rest.splitlines(keepends=True)
            idx = 0
            while idx < len(lines):
                s = lines[idx].strip()
                if (
                    s == ""
                    or s.startswith("//")
                    or s.startswith("apiRouter.Use(")
                    or BEGIN in s
                    or END in s
                    or s == "{"
                ):
                    idx += 1
                    continue
                break
            insert_at = m.end() + sum(len(x) for x in lines[:idx])
            # Match indentation of the next real line if possible
            indent = "\t\t"
            if idx < len(lines):
                lead = re.match(r"^(\s*)", lines[idx])
                if lead and lead.group(1):
                    indent = lead.group(1)
            block = (
                f"{indent}{BEGIN}\n"
                f"{indent}registerDesktopConnectRoutes(apiRouter)\n"
                f"{indent}{END}\n"
            )
            text = text[:insert_at] + block + text[insert_at:]
            changed = True
            notes.append("ok:registerDesktopConnectRoutes")
    else:
        notes.append("skip:registerDesktopConnectRoutes")

    if changed:
        _write(path, text)
    return notes


def patch_automigrate(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    if "DesktopSession{}" in text:
        return ["skip:DesktopSession"]
    if "DB.AutoMigrate(" not in text:
        return ["WARN:no-AutoMigrate"]

    # Insert after AutoMigrate( line, before first &Type{}
    pattern = re.compile(r"(DB\.AutoMigrate\(\s*\n)([ \t]*)&([A-Za-z0-9_]+)\{\}")
    m = pattern.search(text)
    if not m:
        return ["WARN:AutoMigrate-list-shape"]
    indent = m.group(2)
    block = (
        f"{m.group(1)}"
        f"{indent}{BEGIN}\n"
        f"{indent}&DesktopSession{{}},\n"
        f"{indent}{END}\n"
        f"{indent}&{m.group(3)}{{}}"
    )
    text = text[: m.start()] + block + text[m.end() :]
    _write(path, text)
    return ["ok:DesktopSession"]


def patch_user_auth(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    if "desktop_session_id" in text and "YESCHOY_DESKTOP_CONNECT" in text:
        return ["skip:UserAuth"]
    skip = (
        f"\t\t{BEGIN}\n"
        f'\t\tif c.GetInt("id") > 0 && c.GetString("desktop_session_id") != "" {{\n'
        f"\t\t\tc.Next()\n"
        f"\t\t\treturn\n"
        f"\t\t}}\n"
        f"\t\t{END}\n"
    )
    pattern = re.compile(
        r"(func UserAuth\(\)[^\n]*\n(?:.*\n){0,8}?\s*return func\(c \*gin\.Context\) \{\s*\n)",
        re.M,
    )
    m = pattern.search(text)
    if not m:
        return ["WARN:UserAuth-shape"]
    text = text[: m.end()] + skip + text[m.end() :]
    _write(path, text)
    return ["ok:UserAuth"]


def patch_web_router(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    if "/desktop/connect" in text:
        return ["skip:web-connect"]
    m = re.search(r"func SetWebRouter\([^{]*\{", text)
    if not m:
        return ["WARN:SetWebRouter"]
    # Insert right after the opening "{" of SetWebRouter
    insert_at = m.end()
    # consume one following newline if present so we own formatting
    if insert_at < len(text) and text[insert_at] == "\n":
        insert_at += 1
    mw = "middleware.DisableCache(), "
    block = (
        f"\t{BEGIN}\n"
        f'\trouter.GET("/desktop/connect", {mw}controller.ServeDesktopConnectPage)\n'
        f"\t{END}\n"
    )
    text = text[:insert_at] + block + text[insert_at:]
    notes = ["ok:web-connect"]
    import_block = text.split("import (", 1)[-1].split(")", 1)[0] if "import (" in text else ""
    if "controller." in block and "controller" not in import_block:
        notes.insert(0, "WARN:need-controller-import")
    _write(path, text)
    return notes


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print("usage: patch_helpers.py <api_router|automigrate|user_auth|web_router> <file>", file=sys.stderr)
        return 2
    cmd, file = argv[1], Path(argv[2])
    handlers = {
        "api_router": patch_api_router,
        "automigrate": patch_automigrate,
        "user_auth": patch_user_auth,
        "web_router": patch_web_router,
    }
    if cmd not in handlers:
        print("unknown cmd", file=sys.stderr)
        return 2
    notes = handlers[cmd](file)
    for n in notes:
        print(n)
    return 0 if not any(n.startswith("WARN:") and "inserted-maybe" not in n for n in notes) else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
