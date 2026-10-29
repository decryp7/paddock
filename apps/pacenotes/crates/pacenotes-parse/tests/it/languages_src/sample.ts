import { readFile } from "fs";
import * as path from "path";

export interface Shape {
  area(): number;
}

export type Id = number;

export enum Mode {
  Fast,
  Slow,
}

export class Rect implements Shape {
  private w: number;
  constructor(w: number) {
    this.w = w;
  }
  area(): number {
    return this.w * Math.max(this.w, 1);
  }
}

export function load(file: string): void {
  readFile(path.join(file), () => {});
}

const double = (n: number) => n * 2;
const limit = 3;
