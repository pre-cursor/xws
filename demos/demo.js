#!/usr/bin/osascript -l JavaScript
// Run with: osascript -l JavaScript demos/demo.js

const app = Application.currentApplication();
app.includeStandardAdditions = true;
const sys = Application("System Events");

const RETURN = 36;
const DOWN = 125;
const UP = 126;

function typeCommand(cmd, wait) {
  sys.keystroke(cmd);
  delay(0.2);
  sys.keyCode(RETURN);
  delay(wait);
}

// Run xws ls, let it print, then pause so the viewer can catch up
function ls() {
  typeCommand("xws ls", 1.5);
}

const terminal = Application("Terminal");
terminal.activate();
delay(1.5);

// General demo...

const window = terminal.windows[0];
savedTitle = window.customTitle;
window.customTitle = "XWS Navigational Demo";
window.titleDisplaysCustomTitle = true;

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws help", 3.0);
typeCommand("clear", 1.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws", 0.5);

// Arrow keys & Enter inside xws
delay(0.5);
sys.keyCode(DOWN);
delay(0.5);
sys.keyCode(UP);
delay(0.5);
sys.keyCode(RETURN);

delay(1.5);
sys.keyCode(RETURN);

ls();

typeCommand("xws pop", 0.5);

ls();

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws create foo", 0.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws push foo", 1.0);

ls();

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws path foo", 0.5);
typeCommand("xws which foo", 1.0);
delay(1.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws pop", 0.5);

ls();

typeCommand("xws rm foo", 0.5);

ls();

// Python isolation demo...

window.customTitle = "XWS Python Isolation Demo";
window.titleDisplaysCustomTitle = true;

typeCommand("clear", 1.5);
delay(1.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws create foo --profile python", 1.0);
typeCommand("xws create bar --profile python", 0.5);

ls();

sys.keyCode(RETURN);
delay(1.5);

typeCommand("python -c \"import sys; print(f'Hello Python World from {sys.executable}')\"", 0.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws push foo", 1.0);
typeCommand("python -c \"import sys; print(f'Hello Python World from {sys.executable}')\"", 0.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws push bar", 1.0);
typeCommand("python -c \"import sys; print(f'Hello Python World from {sys.executable}')\"", 0.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws pop", 0.5);
typeCommand("python -c \"import sys; print(f'Hello Python World from {sys.executable}')\"", 0.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws pop", 0.5);
typeCommand("python -c \"import sys; print(f'Hello Python World from {sys.executable}')\"", 0.5);

sys.keyCode(RETURN);
delay(1.5);

typeCommand("xws rm foo", 1.0);
typeCommand("xws rm bar", 0.5);

sys.keyCode(RETURN);
delay(1.5);

window.customTitle = savedTitle;
window.titleDisplaysCustomTitle = false;