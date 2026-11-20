import { h } from "../../../sdk";
import { itemAt, rowOfKey, titleOf } from "../../../song/model";
import type { Page } from "../../../song/view";
import { Button } from "../../button";

/** The clip `clip`, `row/name`, whose notes it edits; it fills the height the page gives it. */
export type EditorProps = { page: Page; clip: string };

/** The note editor of one clip, under the tracks. */
export const Editor = ({ page, clip }: EditorProps) => {
  const item = itemAt(page.self.children, clip);
  const track = page.tracks.find((each) => each.name === rowOfKey(clip));
  const name = item?.tool === "timeline" && item.state.title ? item.state.title : track ? titleOf(track) : clip;
  return (
    <col w="full" h="full" fill="background">
      <row w="full" h={32} px={4} gap={2} align="center" border="border" borderSides={[0, 0, 1, 0]}>
        <text style="heading" color="text" truncate>{name}</text>
        <row grow={1} justify="end">
          <Button id="close-editor" label="Close the note editor" icon="x" variant="ghost" size="sm" tip="Close" onPress={() => page.edits.tracks.openPanel(rowOfKey(clip))} />
        </row>
      </row>
      <box grow={1} w="full" fill="well" />
    </col>
  );
};
