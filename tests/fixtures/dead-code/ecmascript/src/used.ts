function usedPrivate(): string {
  return "used";
}

export function used(): string {
  return usedPrivate();
}

export function testOnly(): string {
  return "test";
}
