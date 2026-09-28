"""Print Semgrep JSON results as GitHub Actions annotations."""

import json
import sys


def escape(text: str) -> str:
    """Escape a value for a workflow command (see GitHub's workflow command syntax)."""
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def main(path: str) -> None:
    with open(path, encoding="utf-8") as handle:
        report = json.load(handle)
    for result in report.get("results", []):
        rule = result["check_id"].rsplit(".", 1)[-1]
        message = escape(result["extra"]["message"].strip())
        print(
            f"::error file={result['path']},line={result['start']['line']},"
            f"title=Semgrep {rule}::{message}"
        )
    for error in report.get("errors", []):
        print(f"::warning title=Semgrep error::{escape(str(error.get('message', error)))}")
    print(f"Semgrep: {len(report.get('results', []))} findings, {len(report.get('errors', []))} errors")


if __name__ == "__main__":
    main(sys.argv[1])
