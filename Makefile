SRC_DIR=src
flags=-O2 -Wall -std=c2x -I$(SRC_DIR)

SRCS=$(wildcard $(SRC_DIR)/*.c $(SRC_DIR)/*/*.c)
HEADERS=$(wildcard $(SRC_DIR)/*.h $(SRC_DIR)/*/*.h)
OBJS=$(SRCS:.c=.o)

.PHONY: all test clean

all: klyro

klyro: $(OBJS)
	cc ${flags} $^ -o $@ ${ldflags}

$(SRC_DIR)/%.o: $(SRC_DIR)/%.c $(HEADERS)
	cc ${flags} -c $< -o $@

test: klyro
	python3 -m unittest discover -s tests -p 'test_*.py' -v

clean:
	rm -rf $(SRC_DIR)/*.o $(SRC_DIR)/*/*.o klyro
