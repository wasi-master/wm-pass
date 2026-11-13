// A once-a-second tick for what shows the time passing, such as one-time codes.

/**
 * Runs `tick` just after each second turns, while the page can be seen. A hidden page's timers
 * are slowed to as little as once a minute, so it rests instead, and ticks at once on coming back
 * rather than showing a code that went stale meanwhile. Returns a way to stop.
 */
export function everySecond(tick: () => Promise<void> | void): () => void {
  let stopped = false;
  let turn = 0;
  let next: ReturnType<typeof setTimeout> | undefined;
  const run = async () => {
    clearTimeout(next);
    if (stopped || document.hidden) return;
    const mine = ++turn;
    await tick();
    // A newer run, started by the page coming back into view, carries on in its place.
    if (stopped || mine !== turn || document.hidden) return;
    next = setTimeout(run, 1000 - (Date.now() % 1000) + 10);
  };
  const seen = () => {
    if (!document.hidden) void run();
  };
  document.addEventListener('visibilitychange', seen);
  void run();
  return () => {
    stopped = true;
    clearTimeout(next);
    document.removeEventListener('visibilitychange', seen);
  };
}
