import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "./App";

describe("App smoke", () => {
  it("页头写的是当前页名，不是应用名", () => {
    // 应用名原来占着左上角，但名字在托盘和窗口标题栏已经常驻，
    // 那个位置留给「你在哪一页」更有用
    render(<App />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("监控采集");
    expect(screen.queryByText(/Time Scope/i)).toBeNull();
  });
});
