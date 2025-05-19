# Debugger

The `fastled` app supports full C++ debugging in your browser.

This includes:

  * Breakpoints
  * Stepping
  * Inspecting variables
  * Stack

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