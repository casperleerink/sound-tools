import { h, type Child } from "../sdk";

/** Shown while `shown`, else only while the pointer or the keys are on the `hoverGroup` around it: the quiet rule. */
export const Quiet = ({ shown, children }: { shown: boolean; children?: Child }) => (shown ? <box>{children}</box> : <box reveal>{children}</box>);
