#include "led.h"
#include "driver/gpio.h"

#define LED_GPIO  GPIO_NUM_2

static uint32_t s_level = 0U;

void led_init(void)
{
    gpio_reset_pin(LED_GPIO);
    gpio_set_direction(LED_GPIO, GPIO_MODE_OUTPUT);
}

void led_toggle(void)
{
    s_level = (0U == s_level) ? 1U : 0U;
    gpio_set_level(LED_GPIO, s_level);
}
