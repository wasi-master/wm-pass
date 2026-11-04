<script lang="ts">
  import Btn from './Btn.svelte';
  import Dialog from './Dialog.svelte';
  import Select from './Select.svelte';
  import { pass } from '../engine/pass.svelte';
  import { consumeText } from '../engine/frame';

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
  let cameras = $state<[string, string][]>([]);
  let camera = $state('');
  let stream: MediaStream | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let reading = false;
  let done = false;
  const canvas = document.createElement('canvas');

  $effect(() => {
    void start(camera);
    return stop;
  });

  function stop() {
    if (timer) clearInterval(timer);
    timer = null;
    stream?.getTracks().forEach((t) => t.stop());
    stream = null;
  }

  async function start(deviceId: string) {
    stop();
    problem = null;
    if (!navigator.mediaDevices?.getUserMedia) {
      problem = 'This window cannot use a camera. Choose a picture instead.';
      return;
    }
    try {
      stream = await navigator.mediaDevices.getUserMedia({
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
    if (done) return stop();
    if (video) {
      video.srcObject = stream;
      await video.play().catch(() => undefined);
    }
    if (cameras.length === 0) {
      const all = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'videoinput');
      if (all.length > 1) cameras = all.map((d, n) => [d.deviceId, d.label || `Camera ${n + 1}`]);
    }
    timer = setInterval(() => void frame(), 250);
  }

  /** Decodes what `source` shows, scaled down to at most `longest` pixels across. */
  async function read(source: CanvasImageSource, width: number, height: number, longest: number) {
    const scale = Math.min(1, longest / Math.max(width, height));
    canvas.width = Math.max(1, Math.round(width * scale));
    canvas.height = Math.max(1, Math.round(height * scale));
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (!context) return [];
    context.drawImage(source, 0, 0, canvas.width, canvas.height);
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
    const reply = await pass.engine('qr.read', { width: canvas.width, height: canvas.height, rgba: true }, { blobs: [new Uint8Array(pixels.data.buffer)] });
    pixels.data.fill(0);
    return reply.json.found > 0 ? (JSON.parse(consumeText(reply.secret)) as string[]) : [];
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
      found(await read(video, video.videoWidth, video.videoHeight, 960));
    } catch {
      // A frame that fails to read is just a missed frame.
    }
    reading = false;
  }

  async function picture(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;
    problem = null;
    try {
      const image = await createImageBitmap(file);
      // A screenshot's code can be small, so a picture is read larger than a frame.
      let texts = await read(image, image.width, image.height, 2400);
      if (texts.length === 0) texts = await read(image, image.width, image.height, 1200);
      image.close();
      if (texts.length === 0) problem = 'No QR code was found in that picture.';
      else found(texts);
    } catch {
      problem = 'That picture could not be read.';
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
    {#if cameras.length > 1}
      <Select value={camera} options={cameras} onchange={(id) => (camera = id)} />
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
