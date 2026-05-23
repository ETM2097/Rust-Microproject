#include "led.h"
#include "button.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

void app_main(void)
{
    /* The bug: both init() calls touch GPIO2. The last one is wo takes it. 
       The compiler is silent about it. */
    led_init();
    button_init();

    while (1)
    {
        led_toggle();
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}
