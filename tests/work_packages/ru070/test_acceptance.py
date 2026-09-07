from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def test_configured_action_hierarchy_survives_the_final_css_cascade():
    index_css = read("src/index.css")
    workbench_css = read("src/workbench/workbench-v2.css")
    audit = read("outputs/野菜API-接入体验审计.md")

    deck_start = index_css.index(".connection-action-deck {")
    deck_rule = index_css[deck_start : index_css.index("}", deck_start)]
    assert "width: 100%" in deck_rule
    assert "min-width: 0" in deck_rule

    override_start = workbench_css.index(
        ".configuration-preview-panel:has(.open-connection-control) .setup-apply"
    )
    override = workbench_css[
        override_start : workbench_css.index("}", override_start)
    ]
    assert "background: var(--surface)" in override
    assert "border-color: var(--accent-line)" in override
    assert "color: var(--accent)" in override
    assert "background: var(--subtle)" not in override
    assert "视觉级联" in audit
