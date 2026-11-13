// The generator's settings and its current value.
//
// Passwords are drawn once at the longest length and shown as a prefix, so dragging the length adds
// and removes one character at a time instead of replacing them all. A prefix can miss a class the
// longer value had, so letting go checks for that and draws again if it did.

import { consumeText, pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';
import type { PresetView } from '../engine/types';

export const BUFFER = 64;

export class GenState {
  mode = $state<string>(prefs.v.gen_mode);
  length = $state(Math.min(BUFFER, Math.max(8, prefs.v.gen_length)));
  lower = $state(true);
  upper = $state(true);
  digits = $state(true);
  symbols = $state<boolean>(prefs.v.gen_symbols);
  noAmbiguous = $state<boolean>(prefs.v.gen_no_ambiguous);
  words = $state(prefs.v.gen_words);
  separator = $state('-');
  capitalize = $state(false);
  includeDigit = $state(true);
  syllables = $state(6);
  pin = $state(6);
  pattern = $state('Cvcv-9999');

  private buffer = $state('');
  private whole = $state('');
  private drawnBits = $state(0);

  value = $derived(this.mode === 'password' ? this.buffer.slice(0, this.length) : this.whole);
  bits = $derived(this.mode === 'password' ? (this.drawnBits * this.length) / BUFFER : this.drawnBits);

  private recipe() {
    switch (this.mode) {
      case 'password':
        return { Password: { length: BUFFER, lower: this.lower, upper: this.upper, digits: this.digits, symbols: this.symbols, exclude_ambiguous: this.noAmbiguous } };
      case 'passphrase':
        return { Passphrase: { words: this.words, separator: this.separator, capitalize: this.capitalize, include_digit: this.includeDigit } };
      case 'pronounceable':
        return { Pronounceable: { syllables: this.syllables, separator: '' } };
      case 'pin':
        return { Pin: { length: this.pin } };
      default:
        return { Pattern: this.pattern };
    }
  }

  private prefixIsWhole(candidate: string) {
    const shown = candidate.slice(0, this.length);
    if (this.lower && !/\p{Ll}/u.test(shown)) return false;
    if (this.upper && !/\p{Lu}/u.test(shown)) return false;
    if (this.digits && !/\d/.test(shown)) return false;
    if (this.symbols && !/[^\p{L}\p{N}]/u.test(shown)) return false;
    return true;
  }

  private rolls = 0;

  async roll() {
    if (this.mode === 'password' && !this.lower && !this.upper && !this.digits && !this.symbols) this.lower = true;
    // Only the latest ask is shown: a slower reply for settings since changed is dropped.
    const mine = ++this.rolls;
    for (let i = 0; i < 40; i++) {
      let reply;
      try {
        reply = await pass.engine('gen.generate', this.recipe());
      } catch {
        // A pattern the engine cannot follow, such as an empty one, gives nothing to copy rather
        // than leaving a value made from the pattern before.
        if (mine === this.rolls && this.mode !== 'password') this.whole = '';
        return;
      }
      const text = consumeText(reply.secret);
      if (mine !== this.rolls) return;
      this.drawnBits = Number(reply.json.bits) || 0;
      if (this.mode === 'password') {
        this.buffer = text;
        if (this.prefixIsWhole(text)) return;
      } else {
        this.whole = text;
        return;
      }
    }
  }

  async settle() {
    if (this.mode === 'password' && !this.prefixIsWhole(this.buffer)) await this.roll();
  }

  apply(preset: PresetView) {
    const r = preset.recipe;
    if (r?.Password) {
      const p = r.Password;
      this.mode = 'password';
      this.length = Math.min(BUFFER, Math.max(8, p.length ?? 20));
      this.lower = p.lower ?? true;
      this.upper = p.upper ?? true;
      this.digits = p.digits ?? true;
      this.symbols = p.symbols ?? true;
      this.noAmbiguous = p.exclude_ambiguous ?? false;
    } else if (r?.Passphrase) {
      const p = r.Passphrase;
      this.mode = 'passphrase';
      this.words = p.words ?? 4;
      this.separator = p.separator ?? '-';
      this.capitalize = p.capitalize ?? false;
      this.includeDigit = p.include_digit ?? true;
    } else if (r?.Pronounceable) {
      this.mode = 'pronounceable';
      this.syllables = r.Pronounceable.syllables ?? 6;
    } else if (r?.Pin) {
      this.mode = 'pin';
      this.pin = r.Pin.length ?? 6;
    } else if (typeof r?.Pattern === 'string') {
      this.mode = 'pattern';
      this.pattern = r.Pattern;
    }
  }
}
