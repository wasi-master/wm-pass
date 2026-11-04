// QR codes in pictures and camera frames, decoded by the engine on this device. What a code says
// often holds a secret, such as a one-time code's seed, so it comes back from the engine as its
// secret and is handed straight on.

import { pass } from '../engine/pass.svelte';
import { consumeText } from '../engine/frame';

const canvas = document.createElement('canvas');

/** Decodes what `source` shows, scaled down to at most `longest` pixels across. */
export async function readQr(source: CanvasImageSource, width: number, height: number, longest: number): Promise<string[]> {
  const scale = Math.min(1, longest / Math.max(width, height));
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context) return [];
  context.drawImage(source, 0, 0, canvas.width, canvas.height);
  const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
  const reply = await pass.engine('qr.read', { width: canvas.width, height: canvas.height, rgba: true }, { blobs: [new Uint8Array(pixels.data.buffer)] });
  pixels.data.fill(0);
  context.clearRect(0, 0, canvas.width, canvas.height);
  return reply.json.found > 0 ? (JSON.parse(consumeText(reply.secret)) as string[]) : [];
}

/** Every code in a picture. A screenshot's code can be small, so a picture is read larger than a frame. */
export async function qrInPicture(picture: Blob): Promise<string[]> {
  const image = await createImageBitmap(picture);
  try {
    const texts = await readQr(image, image.width, image.height, 2400);
    return texts.length > 0 ? texts : await readQr(image, image.width, image.height, 1200);
  } finally {
    image.close();
  }
}
