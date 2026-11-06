import { HashRouter, Route, Routes } from "react-router";
import { Home } from "./routes/Home";

export function App() {
  return (
    <HashRouter>
      <Routes>
        <Route path="*" element={<Home />} />
      </Routes>
    </HashRouter>
  );
}
