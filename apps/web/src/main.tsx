import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { App } from "./App"
import { TooltipProvider } from "./components/ui/tooltip"
import "./index.css"

createRoot(document.getElementById("root")!).render(
  <StrictMode><TooltipProvider delay={300}><App /></TooltipProvider></StrictMode>,
)
