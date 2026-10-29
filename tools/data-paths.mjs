// Local owner-supplied assets. No game data belongs in the repository.
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

export const DATA_ROOT = resolve(process.env.U6_DATA_ROOT || join(homedir(), 'Games', 'ultima6'));
export const dataPath = (...parts) => join(DATA_ROOT, ...parts);
export const ORIGINAL = resolve(process.env.U6_ORIGINAL_DIR || dataPath('original'));
export const ARCHIVE = resolve(process.env.U6_ARCHIVE || join(ORIGINAL, 'ultima-vi.zip'));
export const UNPACKED = resolve(process.env.U6_DATA_DIR || dataPath('unpacked'));
export const GAME_BIN = resolve(process.env.U6_GAME_BIN || dataPath('re', 'game.bin'));
export const ORACLE_HOME = resolve(process.env.U6_ORACLE_HOME || dataPath('oracle'));
