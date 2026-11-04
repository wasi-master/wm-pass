#!/usr/bin/env python3
"""Builds data/brands from a local copy of the Simple Icons npm package (CC0).

Usage: gen_brands.py <unpacked package dir> [older package dirs...]

Simple Icons takes a logo out when its owner asks, and the sites people most often save are the
ones this has happened to. Those files were released under CC0, so the well-known sites below can
come from an older package. Nothing else does.

brands.tsv holds what is needed to recognise a site: slug, name, colour, domains, other names.
brand-paths/<first letter>.tsv holds the drawings, so an app with thousands of marks only reads
the handful it shows. Well-known sites come first, because the icon picker lists in file order.
Icons that Simple Icons lists under a licence other than CC0 are left out.
The logos remain the marks of their owners; they are shown only to name the site.
"""
import json, pathlib, re, shutil, sys, unicodedata

# slug, extra domains. The first domain of each row is the one people sign in on.
SITES = """
google google.com,gmail.com,youtube.com,accounts.google.com
github github.com
gitlab gitlab.com
bitbucket bitbucket.org
amazon amazon.com,amazon.co.uk,amazon.de,amazon.in,amazon.ca,amazon.fr,amazon.es,amazon.it,amazon.co.jp
apple apple.com,icloud.com
facebook facebook.com
instagram instagram.com
whatsapp whatsapp.com
messenger messenger.com
x x.com,twitter.com
linkedin linkedin.com
reddit reddit.com
discord discord.com,discordapp.com
telegram telegram.org
signal signal.org
slack slack.com
zoom zoom.us
netflix netflix.com
spotify spotify.com
hulu hulu.com
twitch twitch.tv
steam steampowered.com,steamcommunity.com
epicgames epicgames.com
playstation playstation.com,sonyentertainmentnetwork.com
ea ea.com
battledotnet battle.net,blizzard.com
paypal paypal.com
stripe stripe.com
wise wise.com
revolut revolut.com
coinbase coinbase.com
binance binance.com
visa visa.com
mastercard mastercard.com
americanexpress americanexpress.com
dropbox dropbox.com
box box.com
protonmail proton.me,protonmail.com
protondrive drive.proton.me
fastmail fastmail.com
duckduckgo duckduckgo.com
mozilla mozilla.org
firefoxbrowser firefox.com
notion notion.so
obsidian obsidian.md
evernote evernote.com
trello trello.com
asana asana.com
jira atlassian.net
atlassian atlassian.com
figma figma.com
canva canva.com
adobe adobe.com
sketch sketch.com
cloudflare cloudflare.com
digitalocean digitalocean.com
vercel vercel.com
netlify netlify.com
heroku heroku.com
docker docker.com
npm npmjs.com
pypi pypi.org
stackoverflow stackoverflow.com
hackernews news.ycombinator.com
wordpress wordpress.com
medium medium.com
substack substack.com
tumblr tumblr.com
pinterest pinterest.com
snapchat snapchat.com
tiktok tiktok.com
mastodon mastodon.social
bluesky bsky.app
threads threads.net
quora quora.com
airbnb airbnb.com
booking booking.com
uber uber.com
lyft lyft.com
doordash doordash.com
ubereats ubereats.com
shopify shopify.com
ebay ebay.com
etsy etsy.com
aliexpress aliexpress.com
walmart walmart.com
target target.com
ikea ikea.com
nike nike.com
adidas adidas.com
samsung samsung.com
sony sony.com
nintendo nintendo.com
tesla tesla.com
openai openai.com,chatgpt.com
anthropic anthropic.com,claude.ai
huggingface huggingface.co
kaggle kaggle.com
coursera coursera.org
udemy udemy.com
duolingo duolingo.com
khanacademy khanacademy.org
wikipedia wikipedia.org
yahoo yahoo.com
aol aol.com
vk vk.com
line line.me
wechat wechat.com
skype skype.com
paramountplus paramountplus.com
crunchyroll crunchyroll.com
soundcloud soundcloud.com
bandcamp bandcamp.com
patreon patreon.com
kofi ko-fi.com
buymeacoffee buymeacoffee.com
tailscale tailscale.com
nordvpn nordvpn.com
expressvpn expressvpn.com
mullvad mullvad.net
surfshark surfshark.com
namecheap namecheap.com,nc.me
godaddy godaddy.com
hetzner hetzner.com
ovh ovh.com
linode linode.com
sentry sentry.io
datadog datadoghq.com
grafana grafana.com
postman postman.com,getpostman.com,postman.co
jetbrains jetbrains.com
codeberg codeberg.org
sourceforge sourceforge.net
gitea gitea.com
digikey digikey.com
strava strava.com
fitbit fitbit.com
garmin garmin.com
tripadvisor tripadvisor.com
yelp yelp.com
zillow zillow.com
indeed indeed.com
glassdoor glassdoor.com
upwork upwork.com
fiverr fiverr.com
hubspot hubspot.com
salesforce salesforce.com
zendesk zendesk.com
intercom intercom.com
mailchimp mailchimp.com
sendgrid sendgrid.com
twilio twilio.com
auth0 auth0.com
okta okta.com
bitwarden bitwarden.com
1password 1password.com
lastpass lastpass.com
dashlane dashlane.com
epson epson.com,global.epson.com,epsonconnect.com
freepik freepik.com,freepikcompany.com
gogdotcom gog.com
pubg pubg.com
lastdotfm last.fm
lg lg.com,lgaccount.com
paradoxinteractive paradoxinteractive.com,paradoxplaza.com
stremio stremio.com,strem.io
tplink tp-link.com,tplinkcloud.com
themoviedatabase themoviedb.org
xdadevelopers xda-developers.com,xdaforums.com
supercell supercell.com,brawlstars.com,clashroyale.com
keybase keybase.io
musicbrainz musicbrainz.org
readthedocs readthedocs.org,readthedocs.io
algolia algolia.com
stackexchange stackexchange.com
xiaomi xiaomi.com,mi.com
mega mega.io,mega.nz
replit replit.com,repl.it
porkbun porkbun.com
dribbble dribbble.com
microsoft microsoft.com,live.com,outlook.com,office.com
pokemon pokemon.com
shutterstock shutterstock.com
fedora fedoraproject.org
nexusmods nexusmods.com
monkeytype monkeytype.com
greasyfork greasyfork.org
anki ankiweb.net,apps.ankiweb.net
""".strip().splitlines()

def load(package):
    file = next(p for p in (package / "data/simple-icons.json", package / "_data/simple-icons.json") if p.exists())
    data = json.loads(file.read_text())
    return data["icons"] if isinstance(data, dict) else data


root = pathlib.Path(sys.argv[1])
older = [pathlib.Path(p) for p in sys.argv[2:]]
icons = load(root)

# Places that host logos or code. A logo's source here says nothing about the brand's own site.
NOT_THE_SITE = re.compile(
    r"github|gitlab|wikipedia|wikimedia|twitter|\bx\.com|figma|dribbble|behance|brandfetch|seeklogo|worldvector|"
    r"logo|svg|fontawesome|iconify|icons8|cdn|googleusercontent|gstatic|cloudfront|amazonaws|medium\.com|"
    r"youtube|linkedin|facebook|instagram|npmjs|pypi|imgur|notion\.site|webflow|readthedocs"
)
PREFIX = re.compile(r"^(www|about|brand|brands|press|media|assets|static|design|developers?|docs|help|support|corporate|company|newsroom|home|en|blog)\.")


def slug_of(icon):
    if icon.get("slug"):
        return icon["slug"]
    title = icon["title"].lower().replace("+", "plus").replace(".", "dot").replace("&", "and").replace("đ", "d").replace("ħ", "h").replace("ı", "i").replace("ĸ", "k").replace("ŀ", "l").replace("ł", "l").replace("ß", "ss").replace("ŧ", "t")
    title = unicodedata.normalize("NFD", title)
    return re.sub(r"[^a-z0-9]", "", title)


def own_domain(icon, slug):
    """The brand's own site, if its logo's source plainly is one."""
    match = re.match(r"https?://([^/]+)", icon.get("source", ""))
    if not match:
        return None
    host = match.group(1).lower()
    while PREFIX.match(host):
        host = PREFIX.sub("", host, count=1)
    if NOT_THE_SITE.search(host) or "." not in host:
        return None
    label = re.sub(r"[^a-z0-9]", "", host.split(".")[-2])
    if len(label) < 3 or not (label in slug or slug in label):
        return None
    return host


def other_names(icon):
    aliases = icon.get("aliases") or {}
    names = list(aliases.get("aka", [])) + list(aliases.get("old", []))
    names += [d["title"] for d in aliases.get("dup", [])]
    names += list((aliases.get("loc") or {}).values())
    return [n for n in names if "," not in n and "\t" not in n]


curated = {}
for row in SITES:
    slug, domains = row.split()
    curated[slug] = domains

by_slug = {}
dropped = []
for icon in icons:
    licence = (icon.get("license") or {}).get("type")
    if licence and licence != "CC0-1.0":
        dropped.append(icon["title"])
        continue
    by_slug[slug_of(icon)] = icon

home = {slug: root for slug in by_slug}
for package in older:
    for icon in load(package):
        slug = slug_of(icon)
        if slug in curated and slug not in by_slug and (package / "icons" / f"{slug}.svg").exists():
            by_slug[slug] = icon
            home[slug] = package

order = [s for s in curated if s in by_slug] + sorted(s for s in by_slug if s not in curated)
index, chunks, missing = [], {}, []
for slug in order:
    icon = by_slug[slug]
    svg = home[slug] / "icons" / f"{slug}.svg"
    if not svg.exists():
        missing.append(slug)
        continue
    found = re.search(r'\sd="([^"]+)"', svg.read_text())
    if not found:
        missing.append(slug)
        continue
    domains = curated.get(slug) or own_domain(icon, slug) or ""
    index.append("\t".join([slug, icon["title"], icon["hex"], domains, ",".join(other_names(icon))]))
    chunks.setdefault(slug[0], []).append(f"{slug}\t{found.group(1)}")

target = pathlib.Path("data/brands")
shutil.rmtree(target / "brand-paths", ignore_errors=True)
(target / "brand-paths").mkdir(parents=True)
(target / "brands.tsv").write_text("\n".join(index) + "\n")
for letter, rows in chunks.items():
    (target / "brand-paths" / f"{letter}.tsv").write_text("\n".join(rows) + "\n")
total = sum((target / "brand-paths" / f"{c}.tsv").stat().st_size for c in chunks)
mapped = sum(1 for line in index if line.split("\t")[3])
print(len(index), "brands,", mapped, "with a domain;", (target / "brands.tsv").stat().st_size // 1024, "KB index,", total // 1024, "KB drawings")
print("left out for their licence:", len(dropped), "; no drawing:", missing)
print("from older packages:", sorted(s for s, p in home.items() if p != root))
print("curated names not found:", [s for s in curated if s not in by_slug])
