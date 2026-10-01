// Git-style task entry: the first line is the title, everything after it is
// the description — the shape of a commit message (QA report 6). Paste
//
//     deploy the application on the server
//
//     The application is ready for deployment, and the server is up.
//
// into any "new task" field and you get that title and that description.

/** The API's title limit (`CreateTaskReq.title`, 1–200 chars). */
export const TITLE_MAX = 200;

export type SplitMessage = { title: string; description: string };

export function splitCommitMessage(raw: string): SplitMessage {
  const lines = raw.replace(/\r\n?/g, "\n").split("\n");
  // Leading blank lines aren't a subject.
  while (lines.length > 0 && lines[0]!.trim() === "") lines.shift();
  if (lines.length === 0) return { title: "", description: "" };

  const subject = lines.shift()!.trim();
  // Git's blank separator line (or several) isn't part of the body.
  while (lines.length > 0 && lines[0]!.trim() === "") lines.shift();
  let description = lines.join("\n").replace(/\s+$/, "");

  let title = subject;
  if (title.length > TITLE_MAX) {
    // Too long for a title: keep a readable cut, and don't lose the words —
    // the whole subject leads the description.
    title = `${subject.slice(0, TITLE_MAX - 1).trimEnd()}…`;
    description = description ? `${subject}\n\n${description}` : subject;
  }
  return { title, description };
}

/** True when the text has a body below its first line. */
export function hasBody(raw: string): boolean {
  return splitCommitMessage(raw).description.length > 0;
}
