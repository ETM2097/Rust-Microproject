#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#define LED_GPIO          GPIO_NUM_2
#define BLINK_PERIOD_MS   500U

void app_main(void)
{
    uint32_t led_level = 0U;

    gpio_reset_pin(LED_GPIO);
    gpio_set_direction(LED_GPIO, GPIO_MODE_OUTPUT);

    for (;;)
    {
        gpio_set_level(LED_GPIO, led_level);
        led_level = (0U == led_level) ? 1U : 0U;
        vTaskDelay(pdMS_TO_TICKS(BLINK_PERIOD_MS));
    }
}
