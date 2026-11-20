// What `host.ts` uses of Node and Bun beyond `bun.d.ts`, so `bun check` checks the host with the
// SDK it runs next to.

declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
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

interface BunApi {
  /** Loads each file `filter` matches as `onLoad` says. */
  plugin(plugin: {
    setup(build: { onLoad(options: { filter: RegExp }, load: (file: { path: string }) => { contents: string; loader: "ts" | "tsx" }): void }): void;
  }): void;
}

/** Bun reads stdin line by line as `for await (const line of console)`. */
interface Console {
  [Symbol.asyncIterator](): AsyncIterator<string>;
}
