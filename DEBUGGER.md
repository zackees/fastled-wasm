# Debugger

The `fastled` app supports full C++ debugging in your browser.

This includes:

  * Breakpoints
  * Stepping
  * Inspecting variables
  * Stack


![image](https://github.com/user-attachments/assets/774c61fd-4026-48b8-9f36-60295f5c311d)


## Enabling

  * Install the [C++ Devtools Plugin for Chrome/Brave/Chromium](https://chromewebstore.google.com/detail/cc++-devtools-support-dwa/pdcpmagijalfljmkmjngeonclgbbannb)
  * Compile your sketch with `--debug` flag


## Using

  * Launch debugging mode with `fastled --debug`
  * Open the Devtools panel
    * In Chrome, press `F12`
    * Or right click on the page and select `Inspect`
  * Then click the `Sources` -> `Page`
  * You should now see source code for both fastled core and your sketch.


## Notable features

  * If you use FASTLED_ASSERT(...) and it triggers while devtools is open, the debugger will be invoked and the program will pause at the point the assert fired.
