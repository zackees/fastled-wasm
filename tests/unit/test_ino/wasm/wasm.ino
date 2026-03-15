/// @file    wasm.ino
/// @brief   Simple test sketch for verifying native WASM compilation pipeline

#include <FastLED.h>

#define LED_PIN     3
#define NUM_LEDS    60
#define BRIGHTNESS  128
#define COLOR_ORDER GRB

CRGB leds[NUM_LEDS];

void setup() {
    FastLED.addLeds<WS2812B, LED_PIN, COLOR_ORDER>(leds, NUM_LEDS)
        .setCorrection(TypicalLEDStrip);
    FastLED.setBrightness(BRIGHTNESS);
}

void loop() {
    // Cycle through rainbow colors
    static uint8_t hue = 0;
    fill_rainbow(leds, NUM_LEDS, hue, 7);
    FastLED.show();
    hue++;
    delay(20);
}
