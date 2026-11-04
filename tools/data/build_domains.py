#!/usr/bin/env python3
"""Regenerates data/domains.tsv from the 2FA directory (MIT, 2factorauth and contributors).

    curl -o tfa.json https://api.2fa.directory/v3/tfa.json
    python3 tools/data/build_domains.py tfa.json

Output columns: domain, category or "-", 2FA methods joined by commas or "-", and a fourth, "w",
on rows whose category only Wikidata gives. Those are matched by address, never by name.

Sites the directory files under no heading, or lacks, take their category from games.tsv, then
from data/wikidata/sites.tsv and data/wikidata/games.tsv (written by tools/data/wikidata.py).
domains-extra.tsv overrides all.
"""
import json
import os
import sys

MAP = {
    "banking": "Banking",
    "finance": "Finance", "investing": "Finance", "payments": "Finance",
    "insurance": "Finance", "crowdfunding": "Finance",
    "cryptocurrencies": "Crypto",
    "developer": "Developer", "domains": "Developer", "hosting": "Developer",
    "cloud": "Cloud", "backup": "Cloud", "remote": "Cloud", "iot": "Cloud",
    "vpn": "Cloud", "security": "Cloud", "identity": "Cloud",
    "email": "Email",
    "social": "Social", "communication": "Social",
    "retail": "Shopping", "food": "Shopping", "tickets": "Shopping", "postal": "Shopping",
    "gaming": "Gaming", "betting": "Gaming",
    "entertainment": "Entertainment",
    "marketing": "Work", "task": "Work", "legal": "Work", "creativity": "Work",
    "universities": "Education", "education": "Education",
    "government": "Government",
    "health": "Health",
    "travel": "Travel", "transport": "Travel",
    "utilities": "Utilities",
}

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def merge_games(rows: dict, path: str, *mark: str) -> None:
    """Files a game's own site under Gaming, where nothing before gives it another heading."""
    with open(path) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            _, domains = line.rstrip("\n").split("\t")
            for domain in domains.split(","):
                if domain == "-":
                    continue
                category, methods = rows.get(domain, ("-", "-"))[:2]
                if category == "-":
                    rows[domain] = ("Gaming", methods, *mark)


def merge_wikidata(rows: dict) -> None:
    """Files the organisations Wikidata knows by what they are, where nothing else has."""
    path = os.path.join(ROOT, "data", "wikidata", "sites.tsv")
    with open(path) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            domain, category, _ = line.rstrip("\n").split("\t")
            if rows.get(domain, ("-", "-"))[0] == "-":
                rows[domain] = (category, rows.get(domain, ("-", "-"))[1], "w")


def main() -> None:
    rows = {}
    for _, site in json.load(open(sys.argv[1])):
        category = next((MAP[k] for k in site.get("keywords", []) if k in MAP), "-")
        methods = ",".join(site.get("tfa", [])) or "-"
        for domain in [site["domain"], *site.get("additional-domains", [])]:
            rows[domain.lower()] = (category, methods)
    merge_games(rows, os.path.join(ROOT, "data", "games.tsv"))
    merge_wikidata(rows)
    # Wikidata's games last: a game's page can name its publisher's site, which may be Disney's.
    merge_games(rows, os.path.join(ROOT, "data", "wikidata", "games.tsv"), "w")
    with open(os.path.join(ROOT, "data", "domains-extra.tsv")) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            domain, category = line.rstrip("\n").split("\t")
            rows[domain] = (category, rows.get(domain, ("-", "-"))[1])
    with open(os.path.join(ROOT, "data", "domains.tsv"), "w") as out:
        for domain in sorted(rows):
            out.write("\t".join((domain, *rows[domain])) + "\n")
    print(len(rows), "domains")


if __name__ == "__main__":
    main()
