import { readFile } from "fs";
const path = require("path");

class Rect {
  constructor(w) {
    this.w = w;
  }
  area() {
    return this.w * Math.max(this.w, 1);
  }
}

function load(file) {
  const local = path.join(file);
  readFile(local, () => {});
}

const double = (n) => n * 2;
