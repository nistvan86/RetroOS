#include <stdio.h>
#include <windows.h>

int main(void)
{
    HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
    DWORD count;
    char ch;

    puts("WIN-ECHO-LISTENING");
    do {
        count = 0;
        if (!ReadFile(input, &ch, 1, &count, NULL))
            return 1;
    } while (count == 0);

    putchar(ch);
    puts("WIN-ECHO-OK");
    return 0;
}
