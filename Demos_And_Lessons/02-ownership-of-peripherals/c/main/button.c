#include "button.h"
#include "driver/gpio.h"

/* By accident (or by copy-paste) the button module uses the SAME pin
   as the LED module. In C, the compiler does not warn about this. */
#define BUTTON_GPIO  GPIO_NUM_2

void button_init(void)
{
    gpio_reset_pin(BUTTON_GPIO);
    gpio_set_direction(BUTTON_GPIO, GPIO_MODE_INPUT);
    gpio_set_pull_mode(BUTTON_GPIO, GPIO_PULLUP_ONLY);
}

bool button_pressed(void)
{
    return (0 == gpio_get_level(BUTTON_GPIO));
}
