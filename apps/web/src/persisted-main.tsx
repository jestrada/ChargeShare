import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { PersistedApp } from "./PersistedApp"
import "./index.css"
import "./persisted.css"

createRoot(document.getElementById("root")!).render(<StrictMode><PersistedApp /></StrictMode>)
