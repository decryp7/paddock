#!/usr/bin/env node
import { main } from '../src/notices/cli.mjs';

process.exitCode = main(process.argv.slice(2));
