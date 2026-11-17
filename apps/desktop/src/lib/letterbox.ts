/** The picture's band in a window of `width`×`height` for a video of `aspect` (letterboxed, fitted to the width). */
export function pictureBand(width: number, height: number, aspect: number): { top: number; bottom: number } | null {
  if (!(width > 0 && height > 0 && aspect > 0)) return null;
  const pictureHeight = width / aspect;
  // Less than a few pixels of bar on each side: nothing to light.
  if (pictureHeight >= height - 8) return null;
  const top = (height - pictureHeight) / 2;
  return { top, bottom: top + pictureHeight };
}
