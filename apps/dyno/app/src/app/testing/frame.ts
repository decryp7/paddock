/** Waits for the next animation frame: the shared table draws its rows (a CDK virtual viewport) in the frame after it is given them. */
export function nextFrame(): Promise<void> {
  return new Promise((done) => requestAnimationFrame(() => done()));
}
