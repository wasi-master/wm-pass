<script lang="ts">
  import Btn from './Btn.svelte';
  import Dialog from './Dialog.svelte';
  import Select from './Select.svelte';
  import { qrInPicture, readQr } from './qr';

  // Reads QR codes with the computer's camera, or from a picture. Frames are decoded by the
  // engine on this device and go nowhere else. What a code says often holds a secret, such as
  // a one-time code's seed, so it arrives as the engine's secret and goes straight to `onfound`.
  let {
    title = 'Scan a QR code',
    hint = '',
    onfound,
    onclose,
  }: { title?: string; hint?: string; onfound: (texts: string[]) => void; onclose: () => void } = $props();

  let video = $state<HTMLVideoElement>();
  let picker = $state<HTMLInputElement>();
  let problem = $state<string | null>(null);
  // Why a chosen picture gave nothing. Kept apart from the camera's problem, so the camera still
  // shows and keeps reading after a picture with no code in it.
  let pictureNote = $state<string | null>(null);
  let cameras = $state<[string, string][]>([]);
  let camera = $state('');
  let stream: MediaStream | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let reading = false;
  let done = false;
  let turn = 0;

  $effect(() => {
    void start(camera);
    return () => {
      // A camera still being asked for is let go when it arrives, as when the dialog goes away.
      turn++;
      stop();
    };
  });

  function stop() {
    if (timer) clearInterval(timer);
    timer = null;
    stream?.getTracks().forEach((t) => t.stop());
    stream = null;
  }

  async function start(deviceId: string) {
    stop();
    const mine = ++turn;
    problem = null;
    if (!navigator.mediaDevices?.getUserMedia) {
      problem = 'This window cannot use a camera. Choose a picture instead.';
      return;
    }
    let got: MediaStream;
    try {
      got = await navigator.mediaDevices.getUserMedia({
        video: deviceId ? { deviceId: { exact: deviceId } } : { facingMode: 'environment', width: { ideal: 1280 } },
        audio: false,
      });
    } catch (failure: any) {
      problem =
        failure?.name === 'NotAllowedError'
          ? 'The camera was not allowed. Allow it for WM Pass in the system’s privacy settings, or choose a picture.'
          : 'No camera was found. Choose a picture instead.';
      return;
    }
    // Another camera chosen, or the dialog closed, while this one was asked for: let it go, or
    // the camera stays on with nothing to stop it.
    if (done || mine !== turn) {
      got.getTracks().forEach((t) => t.stop());
      return;
    }
    stream = got;
    if (video) {
      video.srcObject = stream;
      await video.play().catch(() => undefined);
    }
    if (cameras.length === 0) {
      // Only for the menu of cameras: a list that cannot be had must not stop this one reading.
      const all = (await navigator.mediaDevices.enumerateDevices().catch(() => [])).filter((d) => d.kind === 'videoinput');
      if (all.length > 1) cameras = all.map((d, n) => [d.deviceId, d.label || `Camera ${n + 1}`]);
    }
    if (done || mine !== turn) return;
    timer = setInterval(() => void frame(), 250);
  }

  function found(texts: string[]) {
    if (done || texts.length === 0) return;
    done = true;
    stop();
    onfound(texts);
  }

  async function frame() {
    if (reading || done || !video || video.readyState < 2) return;
    reading = true;
    try {
      found(await readQr(video, video.videoWidth, video.videoHeight, 960));
    } catch {
      // A frame that fails to read is just a missed frame.
    }
    reading = false;
  }

  async function picture(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;
    pictureNote = null;
    try {
      const texts = await qrInPicture(file);
      if (texts.length === 0) pictureNote = 'No QR code was found in that picture.';
      else found(texts);
    } catch {
      pictureNote = 'That picture could not be read.';
    }
    (event.target as HTMLInputElement).value = '';
  }
</script>

<Dialog {title} onclose={() => { done = true; stop(); onclose(); }}>
  <div class="stack">
    {#if hint}<p class="dim">{hint}</p>{/if}
    {#if problem}
      <p class="dim">{problem}</p>
    {:else}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video bind:this={video} muted playsinline class="viewfinder"></video>
      <p class="dim small">Hold the code up to the camera. It is read on this device.</p>
    {/if}
    {#if pictureNote}<p class="dim">{pictureNote}</p>{/if}
    {#if cameras.length > 1}
      <Select value={camera} options={cameras} label="Camera" onchange={(id) => (camera = id)} />
    {/if}
    <input bind:this={picker} type="file" accept="image/*" hidden onchange={picture} />
  </div>
  {#snippet foot()}
    <Btn label="Choose a picture" icon="image" onclick={() => picker?.click()} />
    <Btn label="Cancel" onclick={() => { done = true; stop(); onclose(); }} />
  {/snippet}
</Dialog>

<style>
  .viewfinder {
    width: 100%;
    max-height: 60vh;
    border-radius: var(--r-m);
    background: #000;
    transform: scaleX(-1);
  }
</style>
