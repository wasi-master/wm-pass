#!/usr/bin/env python3
"""Pulls data the apps ship from Wikidata (CC0), through the QLever endpoint, which answers the
large queries the Wikidata Query Service times out on.

    python3 tools/data/wikidata.py fetch      # queries into target/wikidata/, a few minutes
    python3 tools/data/wikidata.py build      # writes the files below from that cache

Writes:
    data/wikidata/sites.tsv      domain, category, name: organisations by what they are
    data/wikidata/games.tsv      name, domains: games played online, by how widely they are known
    data/wikidata/apps.tsv       package, name, website: Android apps with an official site
    data/wikidata/brands.tsv     slug, domains: sites for Simple Icons brands

build_domains.py and gen_brands.py read these; the trainer reads the names in sites.tsv.
Hand-written lists beside them always win over anything here.
"""
import collections
import json
import os
import re
import sys
import time
import unicodedata
import urllib.parse
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
CACHE = os.path.join(ROOT, "target", "wikidata")
OUT = os.path.join(ROOT, "data", "wikidata")
ENDPOINT = "https://qlever.dev/api/wikidata"
PSL_URL = "https://publicsuffix.org/list/public_suffix_list.dat"

PREFIXES = """PREFIX wd: <http://www.wikidata.org/entity/>
PREFIX wdt: <http://www.wikidata.org/prop/direct/>
PREFIX wikibase: <http://wikiba.se/ontology#>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
"""

# What an organisation is, by class, matched through subclasses. When an item is several, the
# category earlier in this list wins: a state university is Education before Government.
STRONG = [
    ("Crypto", "Q25401607 Q40186999 Q13479982"),
    ("Banking", "Q22687 Q745877 Q60767305 Q1227557 Q3277997 Q157963 Q848507"),
    ("Health", "Q16917 Q1774898 Q563787 Q831941 Q61676930 Q90298865"),
    ("Education", "Q3918 Q189004 Q38723 Q1336920 Q398141 Q19862851 Q31725945"),
    ("Travel", "Q46970 Q1631129 Q291240 Q118710708 Q217107 Q249556 Q17102188 Q10438042 Q847201"),
    ("Gaming", "Q210167 Q1137109 Q664702 Q770881 Q3550859 Q189409 Q116634 Q85632250"),
    ("Email", "Q1273203 Q327618"),
    ("Social", "Q3220391 Q612825 Q28134845 Q2462003 Q18922759 Q6576792 Q2721136 Q168210"),
    ("Entertainment", "Q63241860 Q723685 Q15590336 Q559856 Q2001305 Q1254874 Q1061197 Q20679540"),
    ("Utilities", "Q1326624 Q2401749 Q1941618 Q11371 Q7258040 Q1058891 Q1266169"),
    ("Shopping", "Q4382945 Q3390477 Q18043413 Q2549179 Q507619 Q18534542 Q18509232 Q3181044 Q125265855"),
    ("Finance", "Q2143354 Q1956140 Q11691 Q4230006 Q1752459 Q42336871 Q2563546 Q319845"),
    ("Cloud", "Q56240391 Q93249 Q1281199"),
    ("Work", "Q1254596 Q167035 Q474157 Q485643 Q580148 Q5412217"),
    ("Developer", "Q5892272 Q1073015 Q891055 Q1334294"),
    ("Government", "Q327333 Q192350 Q3917681 Q7843791"),
]
ORDER = [category for category, _ in STRONG]

# Subclasses that reach a root above but are not what its category means.
NOT = set("""
Q106103510 Q106104678 Q10417670 Q28094186 Q734413 Q126367667 Q58542 Q84326403 Q110643339
Q106040099 Q659541 Q64578911 Q9294168 Q4357531 Q2746884 Q897399 Q285852 Q6498663 Q4348600
Q28464970 Q87410555 Q21685563 Q63198389 Q120061681 Q130411818 Q114083348 Q414147 Q5485547
Q2166013
""".split())

# Places a person or a club gets a page on. Their address says nothing about whose page it is.
HOSTS = set("""
facebook.com instagram.com twitter.com x.com youtube.com linkedin.com tiktok.com t.me vk.com
google.com goo.gl sites.google.com blogspot.com wordpress.com wix.com wixsite.com weebly.com
tumblr.com medium.com substack.com github.com gitlab.com bitbucket.org github.io wikipedia.org
wikimedia.org wikidata.org archive.org webcache.googleusercontent.com bit.ly linktr.ee
yahoo.com naver.com daum.net weibo.com qq.com baidu.com livejournal.com jimdo.com
squarespace.com webnode.com ucoz.ru narod.ru tripod.com angelfire.com geocities.com
google.de apple.com apps.apple.com play.google.com microsoft.com amazon.com about.me
gstatic.com googleusercontent.com googleapis.com amazonaws.com cloudfront.net azurewebsites.net
herokuapp.com appspot.com web.app firebaseapp.com netlify.app vercel.app pages.dev android.com
bing.com opera.com linktr.ee carrd.co notion.site canva.site godaddysites.com business.site
""".split())

# What an organisation does, by industry: a weaker sign than its class, used only alone. Industries
# that also hold the makers of the kit (telecommunications, electric power) are left to the classes.
INDUSTRY = {
    "Banking": "Q806718 Q22687",
    "Finance": "Q837171 Q57774188 Q43183 Q2518196 Q43015 Q29584334 Q4290 Q16319025 Q14864997",
    "Crypto": "Q100269062",
    "Gaming": "Q941594 Q1368944 Q11416",
    "Shopping": "Q126793 Q484847 Q15825471 Q180846 Q11707 Q81799 Q41958 Q11828862 Q12684",
    "Utilities": "Q1951366 Q1326624",
    "Cloud": "Q3510521",
    "Entertainment": "Q289 Q15078788",
    "Travel": "Q49389 Q1495452 Q27686 Q1285245 Q9323634 Q1757562 Q3565868 Q178512 Q2072431",
    "Health": "Q31207 Q130370834 Q15067276",
    "Education": "Q136822 Q8434 Q112166132",
    "Government": "Q112166113 Q29585954 Q112166117",
}

UA = {"User-Agent": "wmpass-data-build/1.0 (https://github.com/wasi-master/wm-pass)",
      "Accept": "application/sparql-results+json"}


def sparql(query):
    data = urllib.parse.urlencode({"query": PREFIXES + query}).encode()
    for attempt in range(6):
        try:
            request = urllib.request.Request(ENDPOINT, data=data, headers=UA)
            with urllib.request.urlopen(request, timeout=600) as response:
                return json.load(response)["results"]["bindings"]
        except Exception as error:  # noqa: BLE001 - the endpoint rate-limits; wait and retry
            print("  retrying:", error, file=sys.stderr)
            time.sleep(10 * (attempt + 1))
    raise SystemExit("the endpoint did not answer")


def values(ids):
    return " ".join("wd:" + i for i in ids.split())


ALIVE = "FILTER NOT EXISTS { ?item wdt:P576 ?gone } FILTER NOT EXISTS { ?item wdt:P582 ?ended }"
# Places are not organisations, whatever classes they reach.
NOT_A_PLACE = "FILTER NOT EXISTS { ?item wdt:P31/wdt:P279* wd:Q56061 } FILTER NOT EXISTS { ?item wdt:P31/wdt:P279* wd:Q486972 }"

QUERIES = {}
for category, ids in STRONG:
    QUERIES["class-" + category] = f"""
SELECT ?item ?class ?site ?links ?label WHERE {{
  VALUES ?root {{ {values(ids)} }}
  ?class wdt:P279* ?root . ?item wdt:P31 ?class . ?item wdt:P856 ?site .
  {ALIVE} {NOT_A_PLACE if category == "Government" else ""}
  OPTIONAL {{ ?item wikibase:sitelinks ?links }}
  OPTIONAL {{ ?item rdfs:label ?label FILTER(LANG(?label) = "en") }}
}}"""
for category, ids in INDUSTRY.items():
    QUERIES["industry-" + category] = f"""
SELECT ?item ?site ?links ?label WHERE {{
  VALUES ?industry {{ {values(ids)} }}
  ?item wdt:P452 ?industry . ?item wdt:P856 ?site . {ALIVE}
  OPTIONAL {{ ?item wikibase:sitelinks ?links }}
  OPTIONAL {{ ?item rdfs:label ?label FILTER(LANG(?label) = "en") }}
}}"""
# Games with a multiplayer mode (online, MMO, player versus player) or released for Android, and the
# services they are played through.
QUERIES["games"] = """
SELECT ?item ?label ?site ?links ?date WHERE {
  ?item wdt:P31/wdt:P279* wd:Q7889 .
  { ?item wdt:P404 ?mode VALUES ?mode { wd:Q6895044 wd:Q175173 wd:Q862490 wd:Q3297989 wd:Q719798 wd:Q116634 } }
  UNION { ?item wdt:P400 wd:Q94 }
  ?item wikibase:sitelinks ?links . FILTER(?links >= 6)
  ?item rdfs:label ?label FILTER(LANG(?label) = "en")
  OPTIONAL { ?item wdt:P856 ?site }
  OPTIONAL { ?item wdt:P577 ?date }
}"""
QUERIES["apps"] = """
SELECT ?item ?package ?label ?site ?links WHERE {
  ?item wdt:P3418 ?package . ?item wikibase:sitelinks ?links .
  FILTER NOT EXISTS { ?item wdt:P31/wdt:P279* wd:Q56061 } FILTER NOT EXISTS { ?item wdt:P31/wdt:P279* wd:Q486972 }
  ?item wdt:P31/wdt:P279* ?kind . VALUES ?kind { wd:Q7397 wd:Q35127 wd:Q4830453 wd:Q783794 wd:Q1668024 }
  ?item rdfs:label ?label FILTER(LANG(?label) = "en")
  OPTIONAL { ?item wdt:P856 ?own }
  OPTIONAL { ?item wdt:P127|wdt:P123|wdt:P178 ?owner . ?owner wdt:P856 ?ownersite }
  BIND(COALESCE(?own, ?ownersite) AS ?site)
}"""
QUERIES["brands"] = """
SELECT ?item ?label ?alt ?site ?links WHERE {
  ?item wdt:P856 ?site . ?item wikibase:sitelinks ?links . FILTER(?links >= 5)
  ?item rdfs:label ?label FILTER(LANG(?label) = "en")
  OPTIONAL { ?item wdt:P1448|wdt:P1813 ?alt }
  FILTER NOT EXISTS { ?item wdt:P31/wdt:P279* wd:Q56061 } FILTER NOT EXISTS { ?item wdt:P31/wdt:P279* wd:Q486972 }
}"""


def fetch(names):
    os.makedirs(CACHE, exist_ok=True)
    for name, query in QUERIES.items():
        if names and name not in names:
            continue
        print("fetching", name)
        rows = sparql(query)
        with open(os.path.join(CACHE, name + ".json"), "w") as fh:
            json.dump(rows, fh)
        print("  ", len(rows), "rows")
    psl = os.path.join(CACHE, "public_suffix_list.dat")
    if not os.path.exists(psl):
        urllib.request.urlretrieve(PSL_URL, psl)


def cached(name):
    with open(os.path.join(CACHE, name + ".json")) as fh:
        return json.load(fh)


def value(row, key, default=""):
    return row[key]["value"] if key in row else default


def qid(row):
    return value(row, "item").rsplit("/", 1)[-1]


class Suffixes:
    """Registrable domains by the Public Suffix List, as the apps compute them."""

    def __init__(self, path):
        self.rules, self.wild, self.exceptions = set(), set(), set()
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line or line.startswith("//"):
                    continue
                rule = line.split()[0].encode("idna").decode() if not line.isascii() else line.split()[0]
                if rule.startswith("!"):
                    self.exceptions.add(rule[1:])
                elif rule.startswith("*."):
                    self.wild.add(rule[2:])
                else:
                    self.rules.add(rule)

    def registrable(self, host):
        labels = host.split(".")
        for i in range(len(labels)):
            candidate = ".".join(labels[i:])
            if candidate in self.exceptions:
                return candidate
            parent = ".".join(labels[i + 1:])
            if candidate in self.rules or (parent and parent in self.wild):
                return ".".join(labels[i - 1:]) if i > 0 else None
        return ".".join(labels[-2:]) if len(labels) >= 2 else None


def host_of(url):
    match = re.match(r"^[a-z][a-z0-9+.-]*://([^/?#:]+)", url.strip(), re.I)
    if not match:
        return None
    host = match.group(1).lower().rstrip(".")
    try:
        host = host.encode("idna").decode() if not host.isascii() else host
    except UnicodeError:
        return None
    if re.fullmatch(r"[0-9.]+", host) or "." not in host:
        return None
    return host


def links(row):
    return int(value(row, "links", "0") or 0)


def build_sites(psl):
    strong = collections.defaultdict(set)
    weak = collections.defaultdict(set)
    sites = collections.defaultdict(set)
    label = {}
    fame = {}
    for category, _ in STRONG:
        for row in cached("class-" + category):
            if value(row, "class").rsplit("/", 1)[-1] in NOT:
                continue
            item = qid(row)
            strong[item].add(category)
            sites[item].add(value(row, "site"))
            label.setdefault(item, value(row, "label"))
            fame[item] = links(row)
    for category in INDUSTRY:
        for row in cached("industry-" + category):
            item = qid(row)
            weak[item].add(category)
            sites[item].add(value(row, "site"))
            label.setdefault(item, value(row, "label"))
            fame[item] = links(row)

    def category_of(item):
        if strong[item]:
            return min(strong[item], key=ORDER.index)
        if len(weak[item]) == 1:
            return next(iter(weak[item]))
        return None

    votes = collections.defaultdict(collections.Counter)
    names = collections.defaultdict(list)
    for item, urls in sites.items():
        category = category_of(item)
        # One with fewer than two Wikipedia articles is too often a stale record, and the list
        # ships inside the apps.
        if not category or fame[item] < 2:
            continue
        for url in urls:
            host = host_of(url)
            domain = host and psl.registrable(host)
            if not domain or domain in HOSTS or host in HOSTS:
                continue
            votes[domain][category] += 1
            names[domain].append((fame[item], label.get(item, "")))
    rows = []
    for domain, counter in votes.items():
        category, count = counter.most_common(1)[0]
        if count / sum(counter.values()) < 0.75:
            continue
        name = max(names[domain])[1]
        rows.append((domain, category, clean(name)))
    rows.sort()
    write("sites.tsv", "domain\tcategory\tname: organisations on Wikidata (CC0) by class or industry",
          ["\t".join(r) for r in rows])
    print(len(rows), "sites;", collections.Counter(r[1] for r in rows).most_common())


def clean(text):
    return re.sub(r"[\t\n,]+", " ", text).strip()


def write(name, header, lines):
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, name), "w") as fh:
        fh.write("# " + header + ". Generated by tools/data/wikidata.py.\n")
        for line in lines:
            fh.write(line + "\n")


def squeeze(text):
    text = unicodedata.normalize("NFKD", text)
    return "".join(c for c in text.lower() if c.isalnum())


def words():
    """Common words of English, which make poor game names: a login called Journey is not one."""
    for path in ("/usr/share/dict/words", "/usr/dict/words"):
        if os.path.exists(path):
            with open(path) as fh:
                return {w.strip().lower() for w in fh if w.strip()}
    return set()


def listed_labels():
    """First labels of the sites filed under something other than Gaming, by hand or here."""
    out = set()
    for path in (os.path.join(ROOT, "data", "domains-extra.tsv"), os.path.join(OUT, "sites.tsv")):
        with open(path) as fh:
            for line in fh:
                if line.startswith("#") or not line.strip():
                    continue
                domain, category = line.rstrip("\n").split("\t")[:2]
                if category != "Gaming":
                    out.add(squeeze(domain.split(".")[0]))
    return out


def build_games(psl):
    dictionary = words()
    # Duolingo is a game to Wikidata and a school to the people who use it.
    taken = listed_labels()
    known = {}
    with open(os.path.join(ROOT, "data", "games.tsv")) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            known[squeeze(line.split("\t")[0])] = True
    best = {}
    for row in cached("games"):
        name = clean(value(row, "label"))
        key = squeeze(name)
        year = value(row, "date")[:4]
        if len(key) < 4 or key in known or key in taken or not name or re.fullmatch(r"Q\d+", name):
            continue
        # Single words a person might name anything else after.
        if " " not in name and name.lower() in dictionary:
            continue
        if year and year.isdigit() and int(year) < 2004:
            continue
        domain = ""
        host = host_of(value(row, "site"))
        if host:
            d = psl.registrable(host)
            if d and d not in HOSTS:
                domain = d
        entry = best.setdefault(key, {"name": name, "links": 0, "domains": set()})
        entry["links"] = max(entry["links"], links(row))
        if domain:
            entry["domains"].add(domain)
    rows = sorted(best.values(), key=lambda e: (-e["links"], e["name"]))
    lines = [f"{e['name']}\t{','.join(sorted(e['domains'])) or '-'}" for e in rows]
    write("games.tsv", "name\tdomains or -: games with an online mode or for phones, on Wikidata (CC0), best known first", lines)
    print(len(lines), "games")


PACKAGE = re.compile(r"^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$")
VARIANT = re.compile(r"[._](tv|androidtv|vr|android_vr|wear|wearos|watch|auto|beta|alpha|dev|debug|canary|nightly|preview|tablet|katniss|leanback)(\b|_|$)", re.I)


def build_apps(psl):
    known, names = set(), set()
    with open(os.path.join(ROOT, "data", "related", "apps.tsv")) as fh:
        for line in fh:
            if not line.startswith("#") and line.strip():
                known.add(line.split("\t")[0])
                names.add(squeeze(line.split("\t")[1]))
    best = {}
    for row in cached("apps"):
        package = value(row, "package").strip()
        name = clean(value(row, "label"))
        if not PACKAGE.match(package) or package in known or not name or re.fullmatch(r"Q\d+", name):
            continue
        # An item's other apps for a television, a headset or a watch, which share its name.
        if VARIANT.search(package):
            continue
        host = host_of(value(row, "site"))
        domain = psl.registrable(host) if host else ""
        entry = best.setdefault(package, {"name": name, "links": -1, "sites": set()})
        if links(row) > entry["links"]:
            entry.update(name=name, links=links(row))
        if domain and domain not in HOSTS:
            entry["sites"].add(domain)
    rows = sorted(best.items(), key=lambda kv: (-kv[1]["links"], kv[0]))
    site = lambda e: min(e["sites"], key=lambda d: (not d.endswith(".com"), len(d), d)) if e["sites"] else ""
    # One app to a name. An item's other apps carry its name too, and the hand-written table
    # already has the main app of the best-known ones: Spotify for Artists is not Spotify.
    lines = []
    for package, entry in sorted(rows, key=lambda kv: (-kv[1]["links"], len(kv[0]), kv[0])):
        if entry["links"] < 3 or squeeze(entry["name"]) in names:
            continue
        names.add(squeeze(entry["name"]))
        lines.append(f"{package}\t{entry['name']}\t{site(entry)}")
    write("apps.tsv", "package\tname\twebsite: Android apps on Wikidata (CC0), best known first", lines)
    print(len(lines), "apps")


# Simple Icons brands whose name Wikidata gives to something better known: the Prisma logo is
# the database toolkit's, not the Finnish shop's.
OTHER_BRANDS = {"prisma", "tele5", "eac", "metro"}


def build_brands(psl):
    """Sites for Simple Icons brands, where a Wikidata item has the brand's exact name and an
    official site whose name is the brand's own. gen_brands.py uses them where it finds none."""
    by_name = collections.defaultdict(list)
    for row in cached("brands"):
        host = host_of(value(row, "site"))
        domain = host and psl.registrable(host)
        if not domain or domain in HOSTS:
            continue
        for name in (value(row, "label"), value(row, "alt")):
            if name:
                by_name[squeeze(name)].append((links(row), qid(row), domain))
    dictionary = words()
    brands = os.path.join(ROOT, "data", "brands", "brands.tsv")
    lines = []
    with open(brands) as fh:
        for line in fh:
            slug, title, *_ = line.rstrip("\n").split("\t")
            if slug in OTHER_BRANDS:
                continue
            found = by_name.get(squeeze(title), [])
            if not found:
                continue
            # A plain word or a few letters names too many things: "Ghost" or "EAC". Those only
            # take the site of an item very widely known by that name.
            fame, top, _ = max(found)
            generic = title.lower() in dictionary or len(squeeze(title)) <= 3
            if generic and fame < 40:
                continue
            mine = sorted({d for _, item, d in found if item == top
                           if squeeze(d.split(".")[0]) in (slug, squeeze(title))
                           or (len(squeeze(d.split(".")[0])) >= 4 and squeeze(d.split(".")[0]) in slug)})
            mine = sorted(mine, key=lambda d: (not d.endswith(".com"), len(d), d))[:4]
            if mine:
                lines.append(f"{slug}\t{','.join(mine)}")
    write("brands.tsv", "slug\tdomains: official sites of Simple Icons brands, on Wikidata (CC0)", lines)
    print(len(lines), "brand sites")


def build():
    psl = Suffixes(os.path.join(CACHE, "public_suffix_list.dat"))
    build_sites(psl)
    build_games(psl)
    build_apps(psl)
    build_brands(psl)


if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in ("fetch", "build"):
        raise SystemExit(__doc__)
    if sys.argv[1] == "fetch":
        fetch(sys.argv[2:])
    else:
        build()
