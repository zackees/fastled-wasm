// Simple Blink example for FastLED WASM
// Blinks all LEDs between two colors

#include <FastLED.h>

#define LED_PIN     3
#define NUM_LEDS    60
#define BRIGHTNESS  128
#define LED_TYPE    WS2812B
#define COLOR_ORDER GRB

CRGB leds[NUM_LEDS];

void setup() {
    FastLED.addLeds<LED_TYPE, LED_PIN, COLOR_ORDER>(leds, NUM_LEDS)
        .setCorrection(TypicalLEDStrip);
    FastLED.setBrightness(BRIGHTNESS);
}

void loop() {
    // First color: red
    fill_solid(leds, NUM_LEDS, CRGB::Red);
    FastLED.show();
    delay(500);

    // Second color: blue
    fill_solid(leds, NUM_LEDS, CRGB::Blue);
    FastLED.show();
    delay(500);
}
