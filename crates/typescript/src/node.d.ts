// What `host.ts` uses of Node and Bun beyond `bun.d.ts`, so `bun check` checks the host with the
// SDK it runs next to.

declare module "node:fs" {
  export function realpathSync(path: string): string;
  export function writeSync(fd: number, data: string): number;
}

declare module "node:path" {
  export function join(...paths: string[]): string;
}

declare const process: {
  cwd(): string;
  argv: string[];
  on(event: "unhandledRejection", listener: (error: unknown) => void): void;
  exit(code: number): never;
};

interface ErrorConstructor {
  stackTraceLimit: number;
}

/** Bun reads stdin line by line as `for await (const line of console)`. */
interface Console {
  [Symbol.asyncIterator](): AsyncIterator<string>;
}
