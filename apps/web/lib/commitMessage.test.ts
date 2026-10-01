import { describe, expect, it } from "vitest";
import { TITLE_MAX, hasBody, splitCommitMessage } from "./commitMessage";

describe("splitCommitMessage", () => {
  it("is just a title when there's one line", () => {
    expect(splitCommitMessage("fix the login button")).toEqual({
      title: "fix the login button",
      description: "",
    });
  });

  it("splits the QA report's example the way git would", () => {
    const msg =
      "deploy the application on the server\n\n" +
      "The application is ready for deployment, and the server is up and ready to host it.";
    expect(splitCommitMessage(msg)).toEqual({
      title: "deploy the application on the server",
      description:
        "The application is ready for deployment, and the server is up and ready to host it.",
    });
  });

  it("doesn't need the blank separator line", () => {
    expect(splitCommitMessage("title\nbody line")).toEqual({
      title: "title",
      description: "body line",
    });
  });

  it("keeps the body's own formatting, minus trailing whitespace", () => {
    const msg = "ship it\n\n- step one\n- step two\n\n  indented code\n\n\n";
    expect(splitCommitMessage(msg).description).toBe("- step one\n- step two\n\n  indented code");
  });

  it("ignores leading blank lines and Windows line endings", () => {
    expect(splitCommitMessage("\r\n\r\n  subject  \r\n\r\nbody\r\n")).toEqual({
      title: "subject",
      description: "body",
    });
  });

  it("is empty for whitespace", () => {
    expect(splitCommitMessage("  \n \n")).toEqual({ title: "", description: "" });
  });

  it("cuts an over-long subject without losing it", () => {
    const subject = "x".repeat(TITLE_MAX + 50);
    const { title, description } = splitCommitMessage(`${subject}\n\nbody`);
    expect(title.length).toBe(TITLE_MAX);
    expect(title.endsWith("…")).toBe(true);
    expect(description).toBe(`${subject}\n\nbody`);
  });

  it("knows when there's a body", () => {
    expect(hasBody("one line")).toBe(false);
    expect(hasBody("one line\n\n")).toBe(false);
    expect(hasBody("one line\n\nand more")).toBe(true);
  });
});
