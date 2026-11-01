import { commands, type ApiError } from './bindings';

export const api = commands;

export class Failure extends Error {
  constructor(public kind: string, message: string) {
    super(message);
  }
}

type Res<T> = { status: 'ok'; data: T } | { status: 'error'; error: ApiError };

/** Unwraps a command result, throwing a Failure on error. */
export async function run<T>(p: Promise<Res<T>>): Promise<T> {
  const r = await p;
  if (r.status === 'ok') return r.data;
  throw new Failure(r.error.kind, r.error.message);
}
