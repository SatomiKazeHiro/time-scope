import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import App from "./App";
import type { StoredEvent } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const focus: StoredEvent = {
  id: "e1",
  timestamp: new Date(2026, 9, 1, 9, 30).getTime(),
  type: "window_focus",
  payload: JSON.stringify({
    process_name: "Code.exe",
    window_title: "main.rs",
    exe_path: "C:\\dev\\Code.exe",
  }),
};

beforeEach(() => {
  invoke.mockReset();
});

describe("App integration", () => {
  it("asks the backend for today's events and renders them on the timeline", async () => {
    invoke.mockResolvedValue([focus]);

    const { container } = render(<App />);

    await waitFor(() => expect(container.querySelectorAll("rect").length).toBe(1));
    expect(invoke).toHaveBeenCalledWith("get_events", { date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/) });
    expect(screen.getByText("1 条事件")).toBeTruthy();
  });

  it("shows an error state instead of throwing when the backend rejects", async () => {
    invoke.mockRejectedValue(new Error("no db"));
    render(<App />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });

  it("shows an empty-day hint rather than a blank bar", async () => {
    invoke.mockResolvedValue([]);
    const { container } = render(<App />);
    await waitFor(() =>
      expect(screen.getByText(/还没有采集到事件/)).toBeTruthy(),
    );
    expect(container.querySelectorAll("rect").length).toBe(0);
  });

  it("re-queries with the newly picked date", async () => {
    invoke.mockResolvedValue([]);
    render(<App />);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByDisplayValue(/^\d{4}-\d{2}-\d{2}$/), {
      target: { value: "2026-01-02" },
    });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_events", { date: "2026-01-02" }),
    );
  });

  it("renders event details when a block is clicked", async () => {
    invoke.mockResolvedValue([focus]);
    const { container } = render(<App />);
    await waitFor(() => expect(container.querySelectorAll("rect").length).toBe(1));

    fireEvent.click(container.querySelector("rect")!);

    await waitFor(() => expect(screen.getByText("切换到窗口")).toBeTruthy());
    expect(screen.getByText("Code.exe")).toBeTruthy();
    expect(screen.getByText("main.rs")).toBeTruthy();
  });

  it("shows a placeholder for an event with a null window title", async () => {
    invoke.mockResolvedValue([
      { ...focus, payload: JSON.stringify({ process_name: "a.exe", window_title: null, exe_path: null }) },
    ]);
    const { container } = render(<App />);
    await waitFor(() => expect(container.querySelectorAll("rect").length).toBe(1));
    fireEvent.click(container.querySelector("rect")!);
    await waitFor(() => expect(screen.getByText("（无标题）")).toBeTruthy());
  });
});
