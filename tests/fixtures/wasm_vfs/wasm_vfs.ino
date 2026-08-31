#include <FastLED.h>
#include "fl/fs/embedded/embedded_fs.h"
#include <stdlib.h>
#include <string.h>

#define LED_PIN 3
#define WIDTH 16
#define HEIGHT 16
#define NUM_LEDS (WIDTH * HEIGHT)

using namespace fl;

CRGB leds[NUM_LEDS];
XYMap screenMap = XYMap::constructRectangularGrid(WIDTH, HEIGHT);

void setup() {
  FileSystem fs;
  if (!fs.begin(getEmbeddedFs())) abort();
  ifstream asset = fs.openRead("data/probe.txt");
  char bytes[15] = {};
  if (!asset.is_open() || asset.read(bytes, sizeof(bytes)).gcount() != sizeof(bytes)
      || fl::memcmp(bytes, "fastled-vfs-ok\n", sizeof(bytes)) != 0) abort();
  FastLED.addLeds<WS2812B, LED_PIN, GRB>(leds, NUM_LEDS)
      .setScreenMap(screenMap);
}

void loop() {
  fill_solid(leds, NUM_LEDS, CRGB::Green);
  FastLED.show();
  delay(20);
}
