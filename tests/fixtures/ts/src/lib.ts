export function used(): void {}
export function unused(): void {}

export interface SupportOptions {
  value: string;
}

export function acceptsSupport(options: SupportOptions): string {
  return options.value;
}
