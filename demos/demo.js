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

Application("Terminal").activate();
delay(1.5);

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
