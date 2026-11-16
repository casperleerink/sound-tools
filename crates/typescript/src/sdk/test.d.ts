// What `bun test` gives a test of a view, for `tsc`. Bun's own types say more.

declare module "bun:test" {
  interface Matchers {
    toBe(expected: unknown): void;
    toEqual(expected: unknown): void;
    toMatchObject(expected: object): void;
    toBeDefined(): void;
    toBeUndefined(): void;
    toContain(expected: unknown): void;
    toBeGreaterThan(expected: number): void;
    toBeLessThan(expected: number): void;
    toThrow(expected?: string | RegExp): void;
    not: Matchers;
  }
  export function expect(value: unknown): Matchers;
  export function test(name: string, run: () => void | Promise<void>): void;
  export function describe(name: string, run: () => void): void;
}
