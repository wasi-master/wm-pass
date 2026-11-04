#!/usr/bin/env python3
"""Regenerates data/domains.tsv from the 2FA directory (MIT, 2factorauth and contributors).

    curl -o tfa.json https://api.2fa.directory/v3/tfa.json
    python3 tools/data/build_domains.py tfa.json

Output columns: domain, category or "-", 2FA methods joined by commas or "-".
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


def merge_games(rows: dict) -> None:
    """Files a game's own site under Gaming, where the directory gives it no other heading."""
    with open(os.path.join(ROOT, "data", "games.tsv")) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            _, domains = line.rstrip("\n").split("\t")
            for domain in domains.split(","):
                if domain == "-":
                    continue
                category, methods = rows.get(domain, ("-", "-"))
                if category == "-":
                    rows[domain] = ("Gaming", methods)


def main() -> None:
    rows = {}
    for _, site in json.load(open(sys.argv[1])):
        category = next((MAP[k] for k in site.get("keywords", []) if k in MAP), "-")
        methods = ",".join(site.get("tfa", [])) or "-"
        for domain in [site["domain"], *site.get("additional-domains", [])]:
            rows[domain.lower()] = (category, methods)
    merge_games(rows)
    with open(os.path.join(ROOT, "data", "domains-extra.tsv")) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            domain, category = line.rstrip("\n").split("\t")
            rows[domain] = (category, rows.get(domain, ("-", "-"))[1])
    with open(os.path.join(ROOT, "data", "domains.tsv"), "w") as out:
        for domain in sorted(rows):
            out.write("%s\t%s\t%s\n" % (domain, *rows[domain]))
    print(len(rows), "domains")


if __name__ == "__main__":
    main()
