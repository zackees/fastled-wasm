from pygments import lex
from pygments.lexers import CppLexer
from pygments.token import Token


def check_cpp_syntax(code):
    try:
        # Tokenize the code to check for basic syntax issues
        for token_type, token_value in lex(code, CppLexer()):
            if token_type == Token.Error:
                print(f"Syntax error detected: {token_value}")
                return False
        print("No syntax errors detected.")
        return True
    except Exception as e:
        print(f"Error during syntax check: {e}")
        return False


def main():
    file_path = input("Enter the path to your C++ file: ")
    try:
        with open(file_path, "r") as file:
            code = file.read()
        if check_cpp_syntax(code):
            print("The file can now be sent to the server.")
        else:
            print("Please fix the syntax errors before sending.")
    except FileNotFoundError:
        print("File not found. Please check the path and try again.")


if __name__ == "__main__":
    main()
