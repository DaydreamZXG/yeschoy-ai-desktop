"""Run the actual billing Vitest scenarios in the read-only acceptance sandbox."""
import runpy
from pathlib import Path

run_selected_vitest = runpy.run_path(
    str(Path(__file__).resolve().parents[1] / "ru012" / "test_acceptance.py")
)["run_selected_vitest"]


def test_receipt_validation(tmp_path: Path) -> None:
    run_selected_vitest("src/billing/comparison.test.ts", tmp_path)


def test_comparison_ui(tmp_path: Path) -> None:
    run_selected_vitest("src/billing/CostComparison.test.tsx", tmp_path)
