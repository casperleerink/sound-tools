// Bun, which runs the tools: the calls tools make most, for `bun check`. Bun's own types are a
// package the project does not have; with it installed (`bun-types`), the runtime leaves this
// file out.
// Anything else of Bun (https://bun.sh/docs) is not checked.

/** A process `Bun.spawn` started. */
interface Subprocess {
  /** Its exit code, once it ends. */
  exited: Promise<number>;
  /** What it writes, with `stdout: "pipe"`, the default: `await new Response(process.stdout).text()`. */
  stdout: ReadableStream<Uint8Array>;
  stderr: ReadableStream<Uint8Array>;
  kill(): void;
}

interface BunApi {
  /** Starts a program, such as `Bun.spawn(["sleep", "2"])`, without waiting for it. */
  spawn(
    command: string[],
    options?: { cwd?: string; stdout?: "pipe" | "inherit" | "ignore"; stderr?: "pipe" | "inherit" | "ignore" },
  ): Subprocess;
  /** Waits `ms` milliseconds. */
  sleep(ms: number): Promise<void>;
  /** A file, relative to `extensions/`: `await Bun.file("words.json").json()`. */
  file(path: string): { text(): Promise<string>; json(): Promise<unknown>; exists(): Promise<boolean> };
  /** Writes a file, relative to `extensions/`. */
  write(path: string, data: string): Promise<number>;
  [other: string]: any;
}

declare const Bun: BunApi;
