export const NEAR_BOTTOM_THRESHOLD_PX = 96;

export function isNearBottom(element: HTMLElement): boolean {
  return element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_BOTTOM_THRESHOLD_PX;
}

export function scrollToLatest(element: HTMLElement): void {
  element.scrollTop = element.scrollHeight;
}
