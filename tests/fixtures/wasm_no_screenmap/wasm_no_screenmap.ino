// Regression fixture for issue #250: a sketch that registers NO screen map in
// setup(). FastLED creates the default layout lazily on the first exported
// frame, so the viewer must re-read layouts after a frame or the canvas stays
// empty. Deliberately keep this free of setScreenMap()/XYMap.
#include <FastLED.h>

#define LED_PIN 3
#define NUM_LEDS 60

CRGB leds[NUM_LEDS];

void setup() {
  FastLED.addLeds<WS2812B, LED_PIN, GRB>(leds, NUM_LEDS);
  FastLED.setBrightness(128);
}

void loop() {
  fill_solid(leds, NUM_LEDS, CRGB::Red);
  FastLED.show();
  delay(20);
}
