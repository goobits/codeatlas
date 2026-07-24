function usedPrivate(): string {
  return "used";
}

function unusedPrivate(): string {
  return "unused";
}

export function used(): string {
  return usedPrivate();
}
