// JSX goes through `h`, so a tool's files import nothing for it. The compiler hands `key` apart.
import { h, type Child } from "./sdk";

export const jsx = (tag: Parameters<typeof h>[0], { children, ...props }: Record<string, any> & { children?: Child }, key?: string) =>
  h(tag, key === undefined ? props : { ...props, key }, ...(children === undefined ? [] : [children]));
export { jsx as jsxs, jsx as jsxDEV };
