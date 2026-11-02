# WM Pass design language

A password manager is opened a dozen times a day for about five seconds at a time: find it, copy it, leave. It also holds things people are afraid of losing. So the app has two jobs that pull in opposite directions. It has to be fast enough to disappear, and it has to feel like a place you would trust with everything.

## Rules

**Animation tells the truth.** The ring around the shield moves because Argon2 is working. The ring on a code is the code's real lifetime. The capsule at the top empties because the clipboard really will be wiped at zero. Nothing moves just to fill time, and every moving thing can be read as information.

**Awe at the thresholds, quiet in the middle.** Unlocking, revealing a secret, rolling a password and locking are events, and they get the full treatment. Scrolling, tapping rows and reading are work, and they stay out of the way: short springs, no stagger after the first arrival.

**Secrets are the hero.** A password is set large in a monospaced face with a slashed zero. Letters, digits and symbols each have their own colour, because people read passwords aloud and type them on televisions.

**One thumb.** Search and add live at the bottom. Everything done daily is within reach of the hand holding the phone, and left-handed people can mirror it.

**Search is the front door.** Typing something that does not exist offers to create it.

**Depth comes from light, not shadow.** Cards are lit from above by a hairline rim. Glass floats over a slow aurora. Each entry tints its own page with its site's colour, so a page can be recognised before it is read.

## Tokens

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

## Signature moments

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

## Keeping it honest

Blur and shaders degrade gracefully: the aurora needs Android 13 and blur needs 12, and below that the same layout stands on gradients and solid glass. Lists never run an infinite animation. The aurora stops when it is off screen, when the screen is scrolled away from it, and when the system says to save power.
