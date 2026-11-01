export function version(): string;
export function presetIr(name: string): string;
export function meshStl(preset: string, resolution: number): Buffer;
export function meshBinary(
  preset: string,
  resolution: number,
  featureAngle?: number,
): Buffer;
export function meshBinaryAsync(
  preset: string,
  resolution: number,
  featureAngle?: number,
): Promise<Buffer>;
export function sample(
  preset: string,
  x: number,
  y: number,
  z: number,
): number;
