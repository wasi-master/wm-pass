# WM Pass design language

People hand this app everything they are afraid of losing, so it cannot look like a weekend demo. The app has two styles, chosen under Settings, Appearance. **Standard** is the default and looks like a well made Android app. **Glass** is the original design, kept as an option for people who like it.

These rules apply to Standard, and the ban list applies to the desktop app and the extension as well.

## Banned

Each of these is something people now read as generated. Check every new screen against this list before it ships.

- Purple or indigo gradients, gradient text, and gradient buttons.
- Glassmorphism, blurred blobs, aurora backgrounds, and glows behind anything.
- Near-black with a single neon accent as the only idea on the page.
- Centred hero with a tagline and two buttons.
- A strip of big numbers with small labels. Health shows one score because it has one score.
- Every section in an identical rounded card with the same shadow, and cards inside cards.
- An icon in a tinted square on every row. Settings is the exception: its tiles are coloured page by page, as in WM Keyboard, so a colour comes to mean a place. They can be changed under Icons.
- Sparkle icons, emoji used as icons, and status dots that mean nothing.
- ALL-CAPS eyebrow labels above headings, numbered template chrome (01, 02, 03), and arrows on buttons.
- Fade-up on every section and lift on every card.
- Marketing voice inside the app: "Your secure digital fortress", "Lock it in", "That is not it".

Swapping one default for another is its own tell. A cream page with a serif headline and terracotta buttons is on this list in spirit, so a replacement has to come from the product.

## What it uses instead

**Platform structure.** Top app bar, navigation bar, floating action button, list rows, outlined fields, dialogs, bottom sheets. Someone who has used any Android app already knows where everything is.

**Material Symbols.** The app's own glyphs are plain Material Symbols by default. Rounded, Sharp and a lighter style are one setting away.

**Everything can be changed.** A choice the design makes is only the default. If two people could reasonably want different things, it belongs under Settings, Customise, and the settings search has to find it. Settings that weaken protection stay out of settings files, so a file from someone else cannot switch them off.

**System type.** The phone's own font, with Material's scale and slightly tighter tracking. A theme may choose another family.

**Depth from surfaces.** Solid colours at a few tonal levels. Rows sit directly on the page with no card, and sections are separated by space and a heading.

**Plain words.** A label names the thing and a button starts with its verb: "Copy password", "Wrong password.", "Discard changes?". Sentences explain only what a person could not guess, and a warning says exactly what will be lost.

**Identity from the product, not from decoration.**

- The shield mark, in its own emerald, on the unlock screen and nowhere else.
- Secrets in a monospaced face with a slashed zero, with digits and symbols in their own colours. People read passwords aloud and type them on televisions.
- Real site marks from Simple Icons in circles, like photos in Contacts. A brand keeps its colour and everything else takes the theme's tone. They can also all share one background, or be letters.
- A code's ring is the code's real lifetime.

## Motion

Motion answers something the person did, or shows something that is really happening.

| Moment | What moves |
|---|---|
| Opening an entry | Its icon and title travel to the new page, the rest follows Material's shared axis |
| Going back | Predictive back, on the versions that have it |
| Opening an editor | Rises a short way, as a full-screen dialog does |
| Copying | The button's glyph turns to a tick and its label says "Copied" for a moment |
| Generating again | The refresh glyph turns once, and the new password replaces the old |
| A code changing | The digits roll, because the code really did change |
| Unlocking | A thin line runs round the shield for as long as the key takes to derive |
| Wrong password | The shield shakes once and turns red, the field goes into its error state |
| Health loading | The bar fills to the score once the check has an answer |
| Holding a row | Its icon flips to a tick |
| A section of the editor | Opens in place, its arrow turning with it |

Nothing loops while idle, nothing staggers in, and "Animations" under Appearance shortens all of it.

## Themes

A theme is a small JSON file: light colours, dark colours, corner size and font. Dracula, Catppuccin, Nord, Solarized, Gruvbox, Tokyo Night, One, Rose Pine, Everforest, Ayu, Kanagawa, Monokai and GitHub ship with the app, and on Android 12 and later the default follows the wallpaper.

- **Colour reference:** each preset uses its published palette, with text nudged where a pair falls under readable contrast.
- **Sharing:** a theme travels as a `wmpass-theme:` code, a QR code or a `.wmtheme` file, and holds nothing from a vault.
- **Importing:** a theme from outside is treated as a stranger's data. It gets a new id, a cleaned name, values within range, and readable text.

## The Glass style

A password manager is opened a dozen times a day for about five seconds at a time: find it, copy it, leave. It also holds things people are afraid of losing. So the app has two jobs that pull in opposite directions. It has to be fast enough to disappear, and it has to feel like a place you would trust with everything.

### Rules

**Animation tells the truth.** The ring around the shield moves because Argon2 is working. The ring on a code is the code's real lifetime. The capsule at the top empties because the clipboard really will be wiped at zero. Nothing moves just to fill time, and every moving thing can be read as information.

**Awe at the thresholds, quiet in the middle.** Unlocking, revealing a secret, rolling a password and locking are events, and they get the full treatment. Scrolling, tapping rows and reading are work, and they stay out of the way: short springs, no stagger after the first arrival.

**Secrets are the hero.** A password is set large in a monospaced face with a slashed zero. Letters, digits and symbols each have their own colour, because people read passwords aloud and type them on televisions.

**One thumb.** Search and add live at the bottom. Everything done daily is within reach of the hand holding the phone, and left-handed people can mirror it.

**Search is the front door.** Typing something that does not exist offers to create it.

**Depth comes from light, not shadow.** Cards are lit from above by a hairline rim. Glass floats over a slow aurora. Each entry tints its own page with its site's colour, so a page can be recognised before it is read.

### Tokens

| Token | Dark | Light |
|---|---|---|
| Ground | `#061311` | `#F4F9F7` |
| Surface 1, 2, 3 | `#0D1C19` `#132522` `#1A2F2A` | `#FFFFFF` `#EEF5F2` `#E4EEEA` |
| Text high, mid, low | `#EAFBF4` `#A3BDB5` `#6C8A81` | `#0A1F1A` `#46605A` `#75908A` |
| Primary | `#34D399` | `#059669` |
| Brand gradient | `#10B981` to `#0D9488` | same |
| Digit, symbol | `#5EEAD4` `#FBBF24` | `#0D9488` `#B45309` |
| Warn, danger | `#FBBF24` `#FB7185` | `#B45309` `#E11D48` |

**Type.** Manrope for headings, Inter for reading, JetBrains Mono for anything secret. Tabular figures wherever numbers change.

**Shape.** Continuous corners throughout, drawn as a superellipse: 28 dp on cards, 18 dp on tiles, full on pills. A circular corner beside a continuous one looks like a mistake, so there are no plain rounded rectangles.

**Motion.** Three springs and nothing else: *snap* for things under a finger, *glide* for things changing place, *lively* for things arriving with some joy. A press scales to 0.97. System animator scale 0 stills all of it, and the app stays fully usable.

### Signature moments

| Moment | What happens |
|---|---|
| Unlock | A comet runs the shield's outline while the key derives. On success the shield fills, then opens as a mask to reveal the vault. A wrong password shakes it red with a rejecting buzz. |
| Lock | The screen folds back into the shield, which latches with a click you can feel. |
| Reveal | Dots scramble through random glyphs and settle left to right into the real characters. Hiding runs it backwards. |
| Copy | Ink spreads from the finger across the row. A capsule drops from the top of the screen with a ring that empties until the clipboard is wiped. Tap it to wipe now. |
| Codes | Digits roll like an odometer when a code changes. The ring turns amber for the last five seconds, and the next code shows beside it so a late copy never fails. |
| Generator | Each roll spins characters from left to right. Dragging the length adds and removes them one at a time, with a tick for each. Pulling down on the password rolls again. |
| Health | The ring sweeps up to the score while the number counts with it. |
| Entry | The row's tile and title fly to their place on the entry's page, which rises tinted in the site's colour. |
| Select | Holding a row flips its tile to a tick, and the dock becomes the action bar. |

### Keeping it honest

Blur and shaders degrade gracefully: the aurora needs Android 13 and blur needs 12, and below that the same layout stands on gradients and solid glass. Lists never run an infinite animation. The aurora stops when it is off screen, when the screen is scrolled away from it, and when the system says to save power.
