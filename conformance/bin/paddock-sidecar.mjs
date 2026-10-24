#!/usr/bin/env node
import { main } from '../src/sidecar/sidecar.mjs';

process.exitCode = main(process.argv.slice(2));
