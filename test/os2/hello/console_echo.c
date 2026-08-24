#include <stdio.h>

int main(void)
{
    int ch;
    puts("OS2-ECHO-LISTENING");
    do {
        ch = getchar();
    } while (ch == EOF);
    putchar(ch);
    puts("OS2-ECHO-OK");
    return 0;
}
