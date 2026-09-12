import * as React from "react";
import { Workspace } from "@/core/Workspace";
import { ExtensionPreview } from "@/routes/ExtensionPreview";
import { Gallery } from "@/routes/Gallery";

function useHashRoute() {
  const [hash, setHash] = React.useState(() => window.location.hash);
  React.useEffect(() => {
    const on = () => setHash(window.location.hash);
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return hash.replace(/^#/, "") || "/";
}

export function App() {
  const route = useHashRoute();
  const ext = route.match(/^\/ext\/([\w-]+)/);
  if (ext?.[1]) return <ExtensionPreview type={ext[1]} />;
  if (route.startsWith("/gallery")) return <Gallery />;
  return <Workspace />;
}
