#!/usr/bin/env node

import { spawnSync } from 'node:child_process'

export const manualTool = true

spawnSync(process.execPath, ['./tools/subprocess.mjs'])
