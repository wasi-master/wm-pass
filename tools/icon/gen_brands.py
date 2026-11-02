#!/usr/bin/env python3
"""Builds data/brands/brands.tsv from a local copy of the Simple Icons npm package (CC0).

Usage: gen_brands.py <unpacked package dir>
Each line is: slug, name, brand colour, comma-separated domains, SVG path on a 24 by 24 grid.
The logos remain the marks of their owners; they are shown only to name the site.
"""
import json, pathlib, re, sys

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
discord discord.com
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
playstation playstation.com
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
namecheap namecheap.com
godaddy godaddy.com
hetzner hetzner.com
ovh ovh.com
linode linode.com
sentry sentry.io
datadog datadoghq.com
grafana grafana.com
postman postman.com
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
""".strip().splitlines()

root = pathlib.Path(sys.argv[1])
data = json.loads((root / "_data/simple-icons.json").read_text())
icons = data["icons"] if isinstance(data, dict) else data


def slug_of(icon):
    if icon.get("slug"):
        return icon["slug"]
    title = icon["title"].lower().replace("+", "plus").replace(".", "dot").replace("&", "and")
    return re.sub(r"[^a-z0-9]", "", title)


by_slug = {slug_of(i): i for i in icons}
out, missing = [], []
for row in SITES:
    slug, domains = row.split()
    svg = root / "icons" / f"{slug}.svg"
    icon = by_slug.get(slug)
    if icon is None or not svg.exists():
        missing.append(slug)
        continue
    path = re.search(r'\sd="([^"]+)"', svg.read_text()).group(1)
    out.append("\t".join([slug, icon["title"], icon["hex"], domains, path]))
target = pathlib.Path("data/brands/brands.tsv")
target.parent.mkdir(parents=True, exist_ok=True)
target.write_text("\n".join(out) + "\n")
print(len(out), "brands,", target.stat().st_size // 1024, "KB; missing:", " ".join(missing))
