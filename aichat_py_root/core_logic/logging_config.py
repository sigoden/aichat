import logging
import sys

def setup_logging(log_level_str="INFO", log_to_file=False, log_file="aichat_app.log"):
    '''
    Configures basic logging for the application.
    '''
    log_level = getattr(logging, log_level_str.upper(), logging.INFO)
    
    formatter = logging.Formatter(
        '%(asctime)s - %(name)s - %(levelname)s - %(message)s',
        datefmt='%Y-%m-%d %H:%M:%S'
    )

    # Configure root logger
    # logging.basicConfig(level=log_level, format='%(asctime)s - %(name)s - %(levelname)s - %(message)s', datefmt='%Y-%m-%d %H:%M:%S')

    # Get the root logger
    logger = logging.getLogger()
    logger.setLevel(log_level)

    # Clear existing handlers (if any, useful for reconfiguration)
    # for handler in logger.handlers[:]:
    #     logger.removeHandler(handler)

    # Console Handler
    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setFormatter(formatter)
    logger.addHandler(console_handler)

    if log_to_file:
        file_handler = logging.FileHandler(log_file)
        file_handler.setFormatter(formatter)
        logger.addHandler(file_handler)
        logging.info(f"Logging also to file: {log_file}")
    
    logging.info(f"Logging initialized with level {log_level_str}")

if __name__ == '__main__':
    # Example usage:
    setup_logging(log_level_str="DEBUG", log_to_file=True, log_file="example.log")
    logging.debug("This is a debug message.")
    logging.info("This is an info message.")
    logging.warning("This is a warning message.")
    sub_logger = logging.getLogger("my_module")
    sub_logger.info("This is an info message from my_module.")
